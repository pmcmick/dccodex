#![cfg(not(target_os = "windows"))]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use codex_core::features::Feature;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::fs_wait;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use responses::ev_assistant_message;
use responses::ev_completed;
use responses::ev_response_created;
use responses::sse;
use responses::start_mock_server;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_three_requests_and_instructions() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let sse1 = sse(vec![ev_assistant_message("m1", "Done"), ev_completed("r1")]);

    responses::mount_sse_once(&server, sse1).await;

    let notify_dir = TempDir::new()?;
    // write a script to the notify that touches a file next to it
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
payload_path="$(dirname "${0}")/notify.txt"
tmp_path="${payload_path}.tmp"
echo -n "${@: -1}" > "${tmp_path}"
mv "${tmp_path}" "${payload_path}""#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.txt");
    let notify_script_str = notify_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| cfg.notify = Some(vec![vec![notify_script_str]]))
        .build(&server)
        .await?;

    // 1) Normal user input – should hit server once.
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello world".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // We fork the notify script, so we need to wait for it to write to the file.
    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let notify_payload_raw = tokio::fs::read_to_string(&notify_file).await?;
    let payload: Value = serde_json::from_str(&notify_payload_raw)?;

    assert_eq!(payload["type"], json!("agent-turn-complete"));
    assert_eq!(payload["input-messages"], json!(["hello world"]));
    assert_eq!(payload["last-assistant-message"], json!("Done"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_submit_hook_appends_prompt_text_to_request() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("append-hook.sh");
    std::fs::write(
        &hook_script,
        r#"#!/bin/bash
set -euo pipefail
printf '%s\n' '{"append_prompt_text":"hook-added context"}'
"#,
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_user_prompt_submit = Some(vec![vec![hook_script_str]]);
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello world".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request = response_mock.single_request();
    assert_eq!(
        request.message_input_texts("user"),
        vec!["hello world".to_string(), "hook-added context".to_string()]
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_submit_hook_switches_turn_to_plan_mode() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("plan-mode-hook.sh");
    std::fs::write(
        &hook_script,
        r#"#!/bin/bash
set -euo pipefail
printf '%s\n' '{"switch_to_plan_mode":true}'
"#,
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_user_prompt_submit = Some(vec![vec![hook_script_str]]);
            cfg.model_reasoning_effort = Some(ReasoningEffort::Low);
            cfg.features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "please plan this change".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request_body = response_mock.single_request().body_json();
    assert_eq!(
        request_body
            .get("reasoning")
            .and_then(|value| value.get("effort"))
            .and_then(|value| value.as_str()),
        Some("medium")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_hooks_emit_json_payloads_on_configure_and_shutdown() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let session_start_script = hook_dir.path().join("session-start-hook.sh");
    let session_start_file = hook_dir.path().join("session-start.json");
    std::fs::write(
        &session_start_script,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={session_start_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(
        &session_start_script,
        std::fs::Permissions::from_mode(0o755),
    )?;

    let session_shutdown_script = hook_dir.path().join("session-shutdown-hook.sh");
    let session_shutdown_file = hook_dir.path().join("session-shutdown.json");
    std::fs::write(
        &session_shutdown_script,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={session_shutdown_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(
        &session_shutdown_script,
        std::fs::Permissions::from_mode(0o755),
    )?;

    let session_start_script_str = session_start_script.to_str().unwrap().to_string();
    let session_shutdown_script_str = session_shutdown_script.to_str().unwrap().to_string();

    let TestCodex {
        codex,
        session_configured,
        ..
    } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_session_start = Some(vec![vec![session_start_script_str]]);
            cfg.notify_on_session_shutdown = Some(vec![vec![session_shutdown_script_str]]);
        })
        .build(&server)
        .await?;

    fs_wait::wait_for_path_exists(&session_start_file, Duration::from_secs(5)).await?;
    let session_start_payload: Value =
        serde_json::from_str(&tokio::fs::read_to_string(&session_start_file).await?)?;
    assert_eq!(
        session_start_payload["hook_event"]["event_type"],
        json!("session_start")
    );
    assert_eq!(
        session_start_payload["hook_event"]["thread_id"],
        json!(session_configured.session_id)
    );
    assert_eq!(
        session_start_payload["hook_event"]["model"],
        json!(session_configured.model)
    );
    assert_eq!(
        session_start_payload["hook_event"]["cwd"],
        json!(session_configured.cwd)
    );

    codex.submit(Op::Shutdown).await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::ShutdownComplete)).await;

    fs_wait::wait_for_path_exists(&session_shutdown_file, Duration::from_secs(5)).await?;
    let session_shutdown_payload: Value =
        serde_json::from_str(&tokio::fs::read_to_string(&session_shutdown_file).await?)?;
    assert_eq!(
        session_shutdown_payload["hook_event"]["event_type"],
        json!("session_shutdown")
    );
    assert_eq!(
        session_shutdown_payload["hook_event"]["thread_id"],
        json!(session_configured.session_id)
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_response_completed_hook_includes_proposed_plan() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let plan_message = "Intro\n<proposed_plan>\n- Step 1\n- Step 2\n</proposed_plan>\nDone";
    responses::mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", plan_message),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("response-completed-hook.sh");
    let payload_file = hook_dir.path().join("response-completed.json");
    std::fs::write(
        &hook_script,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={payload_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_model_response_completed = Some(vec![vec![hook_script_str]]);
            cfg.features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "please plan this".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    fs_wait::wait_for_path_exists(&payload_file, Duration::from_secs(5)).await?;
    let payload: Value = serde_json::from_str(&tokio::fs::read_to_string(&payload_file).await?)?;
    assert_eq!(
        payload["hook_event"]["event_type"],
        json!("after_model_response_completed")
    );
    assert_eq!(
        payload["hook_event"]["proposed_plan"],
        json!("- Step 1\n- Step 2")
    );
    assert_eq!(payload["hook_event"]["needs_follow_up"], json!(false));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn before_model_request_hook_emits_request_metadata() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("before-model-request-hook.sh");
    let payload_file = hook_dir.path().join("before-model-request.json");
    std::fs::write(
        &hook_script,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={payload_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex {
        codex,
        session_configured,
        ..
    } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_before_model_request = Some(vec![vec![hook_script_str]]);
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "inspect the request".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    fs_wait::wait_for_path_exists(&payload_file, Duration::from_secs(5)).await?;
    let payload: Value = serde_json::from_str(&tokio::fs::read_to_string(&payload_file).await?)?;
    assert_eq!(
        payload["hook_event"]["event_type"],
        json!("before_model_request")
    );
    assert_eq!(
        payload["hook_event"]["thread_id"],
        json!(session_configured.session_id)
    );
    assert_eq!(
        payload["hook_event"]["model"],
        json!(session_configured.model)
    );
    assert_eq!(payload["hook_event"]["sampling_request_index"], json!(1));
    assert_eq!(
        payload["hook_event"]["input_messages"],
        json!(["inspect the request"])
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compaction_hook_emits_remote_lifecycle_payloads() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    responses::mount_compact_user_history_with_summary_once(&server, "Compacted summary").await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("compaction-hook.sh");
    let payload_file = hook_dir.path().join("compaction.jsonl");
    std::fs::write(
        &hook_script,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={payload_file:?}
printf '%s\n' "${{@: -1}}" >> "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_compaction = Some(vec![vec![hook_script_str]]);
        })
        .build(&server)
        .await?;

    codex.submit(Op::Compact).await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    fs_wait::wait_for_path_exists(&payload_file, Duration::from_secs(5)).await?;
    let payloads = tokio::fs::read_to_string(&payload_file)
        .await?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(payloads.len(), 2);
    assert_eq!(payloads[0]["hook_event"]["event_type"], json!("compaction"));
    assert_eq!(payloads[0]["hook_event"]["trigger"], json!("manual"));
    assert_eq!(payloads[0]["hook_event"]["strategy"], json!("remote"));
    assert_eq!(payloads[0]["hook_event"]["status"], json!("started"));
    assert_eq!(payloads[0]["hook_event"]["error"], Value::Null);
    assert_eq!(payloads[1]["hook_event"]["event_type"], json!("compaction"));
    assert_eq!(payloads[1]["hook_event"]["trigger"], json!("manual"));
    assert_eq!(payloads[1]["hook_event"]["strategy"], json!("remote"));
    assert_eq!(payloads[1]["hook_event"]["status"], json!("completed"));
    assert_eq!(payloads[1]["hook_event"]["error"], Value::Null);
    assert_eq!(
        payloads[0]["hook_event"]["turn_id"],
        payloads[1]["hook_event"]["turn_id"]
    );

    Ok(())
}
