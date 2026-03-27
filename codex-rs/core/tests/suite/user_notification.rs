#![cfg(not(target_os = "windows"))]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use codex_features::Feature;
use codex_protocol::ThreadId;
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
use responses::ev_function_call;
use responses::ev_response_created;
use responses::mount_sse_sequence;
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
async fn explicit_planning_language_switches_turn_to_plan_mode() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let TestCodex { codex, .. } = test_codex()
        .with_config(|cfg| {
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
                text: "I'd like to create a plan before we implement this change.".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;

    let warning = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    assert!(matches!(
        warning,
        EventMsg::Warning(warning)
            if warning
                .message
                .contains("Automatically switched this turn to Plan mode")
    ));
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
async fn complex_request_switches_turn_to_plan_mode() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let TestCodex { codex, .. } = test_codex()
        .with_config(|cfg| {
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
                text: "We need an end-to-end migration with rollout steps and a multi-file refactor across the request pipeline.".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;

    let warning = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    assert!(matches!(
        warning,
        EventMsg::Warning(warning)
            if warning
                .message
                .contains("request looks complex enough to benefit from planning")
    ));
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
async fn plan_implementation_completed_hook_fires_only_for_initial_child_task() -> anyhow::Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("plan-implementation-completed.sh");
    std::fs::write(
        &hook_script,
        r#"#!/bin/bash
set -euo pipefail
payload_path="$(dirname "$0")/plan-implementation-completed.jsonl"
printf '%s\n' "${@: -1}" >> "$payload_path"
"#,
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();
    let parent_thread_id = ThreadId::new();

    let test = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_plan_implementation_completed = Some(vec![vec![hook_script_str]]);
        })
        .build(&server)
        .await?;
    let codex = test
        .thread_manager
        .start_thread_with_parent(
            test.config.clone(),
            Vec::new(),
            false,
            None,
            Some(parent_thread_id),
        )
        .await?
        .thread;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "Implement the finalized plan.".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "Make a small follow-up refinement.".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let notify_file = hook_dir.path().join("plan-implementation-completed.jsonl");
    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let payloads_raw = tokio::fs::read_to_string(&notify_file).await?;
    let payloads = payloads_raw
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(payloads.len(), 1);
    assert_eq!(
        payloads[0]["hook_event"]["event_type"],
        json!("plan_implementation_completed")
    );
    assert_eq!(
        payloads[0]["hook_event"]["parent_thread_id"],
        json!(parent_thread_id.to_string())
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
async fn model_response_created_hook_emits_request_metadata() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    responses::mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("response-created-hook.sh");
    let payload_file = hook_dir.path().join("response-created.json");
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
            cfg.notify_on_model_response_created = Some(vec![vec![hook_script_str]]);
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "inspect response creation".into(),
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
        json!("after_model_response_created")
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

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_hook_can_deny_tool_execution() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "call-shell-denied";
    let arguments = json!({
        "command": "printf 'real tool should not run'",
        "timeout_ms": 2_000,
        "login": false,
    })
    .to_string();
    let completion_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "shell_command", &arguments),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("pre-tool-deny.sh");
    let payload_file = hook_dir.path().join("pre-tool-deny.json");
    std::fs::write(
        &hook_script,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={payload_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
printf '%s\n' '{{"decision":"deny","message":"blocked by hook"}}'
"#
        ),
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_pre_tool_use = Some(vec![vec![hook_script_str]]);
            cfg.include_apply_patch_tool = true;
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run a shell command".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    fs_wait::wait_for_path_exists(&payload_file, Duration::from_secs(5)).await?;
    let payload: Value = serde_json::from_str(&tokio::fs::read_to_string(&payload_file).await?)?;
    assert_eq!(payload["hook_event"]["event_type"], json!("pre_tool_use"));
    assert_eq!(payload["hook_event"]["tool_name"], json!("shell_command"));
    assert_eq!(payload["hook_event"]["call_id"], json!(call_id));

    let request = completion_mock
        .last_request()
        .expect("completion request exists");
    let (content, success) = request
        .function_call_output_content_and_success(call_id)
        .expect("function_call_output present");
    assert_eq!(content, Some("blocked by hook".to_string()));
    assert_eq!(success, Some(false));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_hook_can_replace_tool_output() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "call-shell-replaced";
    let arguments = json!({
        "command": "printf 'real tool should not run'",
        "timeout_ms": 2_000,
        "login": false,
    })
    .to_string();
    let completion_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "shell_command", &arguments),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let hook_dir = TempDir::new()?;
    let hook_script = hook_dir.path().join("pre-tool-replace.sh");
    std::fs::write(
        &hook_script,
        r#"#!/bin/bash
set -euo pipefail
printf '%s\n' '{"decision":"replace","output":"synthetic tool result","success":true}'
"#,
    )?;
    std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755))?;
    let hook_script_str = hook_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_pre_tool_use = Some(vec![vec![hook_script_str]]);
            cfg.include_apply_patch_tool = true;
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run a shell command".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request = completion_mock
        .last_request()
        .expect("completion request exists");
    let (content, success) = request
        .function_call_output_content_and_success(call_id)
        .expect("function_call_output present");
    assert_eq!(content, Some("synthetic tool result".to_string()));
    assert_eq!(success, Some(true));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_success_and_failure_hooks_emit_distinct_payloads() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let success_call_id = "call-shell-success";
    let failure_call_id = "call-shell-failure";
    let success_arguments = json!({
        "command": "printf 'tool success'",
        "timeout_ms": 2_000,
        "login": false,
    })
    .to_string();
    let failure_arguments = json!({
        "command": "printf 'tool failure'; exit 7",
        "timeout_ms": 2_000,
        "login": false,
    })
    .to_string();
    mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(success_call_id, "shell_command", &success_arguments),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
            sse(vec![
                ev_response_created("resp-3"),
                ev_function_call(failure_call_id, "shell_command", &failure_arguments),
                ev_completed("resp-3"),
            ]),
            sse(vec![
                ev_assistant_message("msg-2", "done"),
                ev_completed("resp-4"),
            ]),
        ],
    )
    .await;

    let hook_dir = TempDir::new()?;
    let success_hook = hook_dir.path().join("post-tool-success.sh");
    let success_file = hook_dir.path().join("post-tool-success.json");
    std::fs::write(
        &success_hook,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={success_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(&success_hook, std::fs::Permissions::from_mode(0o755))?;

    let failure_hook = hook_dir.path().join("tool-failure.sh");
    let failure_file = hook_dir.path().join("tool-failure.json");
    std::fs::write(
        &failure_hook,
        format!(
            r#"#!/bin/bash
set -euo pipefail
payload_path={failure_file:?}
tmp_path="${{payload_path}}.tmp"
echo -n "${{@: -1}}" > "${{tmp_path}}"
mv "${{tmp_path}}" "${{payload_path}}"
"#
        ),
    )?;
    std::fs::set_permissions(&failure_hook, std::fs::Permissions::from_mode(0o755))?;

    let success_hook_str = success_hook.to_str().unwrap().to_string();
    let failure_hook_str = failure_hook.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| {
            cfg.notify_on_post_tool_use_success = Some(vec![vec![success_hook_str]]);
            cfg.notify_on_tool_failure = Some(vec![vec![failure_hook_str]]);
            cfg.include_apply_patch_tool = true;
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run a successful shell command".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run a failing shell command".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    fs_wait::wait_for_path_exists(&success_file, Duration::from_secs(5)).await?;
    let success_payload: Value =
        serde_json::from_str(&tokio::fs::read_to_string(&success_file).await?)?;
    assert_eq!(
        success_payload["hook_event"]["event_type"],
        json!("post_tool_use_success")
    );
    assert_eq!(
        success_payload["hook_event"]["call_id"],
        json!(success_call_id)
    );
    assert_eq!(
        success_payload["hook_event"]["tool_name"],
        json!("shell_command")
    );

    fs_wait::wait_for_path_exists(&failure_file, Duration::from_secs(5)).await?;
    let failure_payload: Value =
        serde_json::from_str(&tokio::fs::read_to_string(&failure_file).await?)?;
    assert_eq!(
        failure_payload["hook_event"]["event_type"],
        json!("tool_failure")
    );
    assert_eq!(
        failure_payload["hook_event"]["call_id"],
        json!(failure_call_id)
    );
    assert_eq!(
        failure_payload["hook_event"]["tool_name"],
        json!("shell_command")
    );
    assert!(
        failure_payload["hook_event"]["error_preview"]
            .as_str()
            .is_some_and(|text| text.contains("Exit code: 7"))
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
