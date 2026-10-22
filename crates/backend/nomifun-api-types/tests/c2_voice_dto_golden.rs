use std::path::PathBuf;
use nomifun_api_types::voice::{
    ModelInstallState, VoiceModelDeleteRequest, VoiceModelDeleteResponse, VoiceModelFileDetail,
    VoiceModelInstallRequest, VoiceModelInstallResponse, VoiceModelProgressEvent,
    VoiceModelStatusResponse,
};
use serde_json::json;

#[test]
fn test_c2_01_status_response_golden() {
    let response = VoiceModelStatusResponse {
        state: ModelInstallState::NotInstalled,
        models_dir: PathBuf::from("/var/lib/ai-mate/models"),
        total_size_bytes: 0,
        files: vec![
            VoiceModelFileDetail {
                path: "kws/encoder.int8.onnx".to_string(),
                size_bytes: 0,
                expected_sha256: Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string()),
                actual_sha256: None,
                exists: false,
            },
            VoiceModelFileDetail {
                path: "vad/silero_vad.onnx".to_string(),
                size_bytes: 0,
                expected_sha256: Some("a1b2c3d4e5f60718293a4b5c6d7e8f90123456789abcdef0123456789abcdef0".to_string()),
                actual_sha256: None,
                exists: false,
            },
        ],
    };

    let expected_json = json!({
        "state": {
            "status": "NotInstalled"
        },
        "models_dir": "/var/lib/ai-mate/models",
        "total_size_bytes": 0,
        "files": [
            {
                "path": "kws/encoder.int8.onnx",
                "size_bytes": 0,
                "expected_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "actual_sha256": null,
                "exists": false
            },
            {
                "path": "vad/silero_vad.onnx",
                "size_bytes": 0,
                "expected_sha256": "a1b2c3d4e5f60718293a4b5c6d7e8f90123456789abcdef0123456789abcdef0",
                "actual_sha256": null,
                "exists": false
            }
        ]
    });

    let value = serde_json::to_value(&response).expect("serialize status response");
    assert_eq!(value, expected_json);

    let deserialized: VoiceModelStatusResponse =
        serde_json::from_value(expected_json).expect("deserialize status response");
    assert_eq!(deserialized, response);
}

#[test]
fn test_c2_02_install_request_golden() {
    let req = VoiceModelInstallRequest {
        prefer_mirror: Some("modelscope".to_string()),
        models_dir: Some(PathBuf::from("/var/lib/ai-mate/models")),
        force: None,
    };

    let expected_json = json!({
        "prefer_mirror": "modelscope",
        "models_dir": "/var/lib/ai-mate/models"
    });

    let value = serde_json::to_value(&req).expect("serialize install request");
    assert_eq!(value, expected_json);

    let deserialized: VoiceModelInstallRequest =
        serde_json::from_value(expected_json).expect("deserialize install request");
    assert_eq!(deserialized, req);
}

#[test]
fn test_c2_03_install_response_golden() {
    let resp = VoiceModelInstallResponse {
        accepted: true,
        state: ModelInstallState::ProbingSource,
        message: Some("Probing download mirrors".to_string()),
    };

    let expected_json = json!({
        "accepted": true,
        "state": {
            "status": "ProbingSource"
        },
        "message": "Probing download mirrors"
    });

    let value = serde_json::to_value(&resp).expect("serialize install response");
    assert_eq!(value, expected_json);

    let deserialized: VoiceModelInstallResponse =
        serde_json::from_value(expected_json).expect("deserialize install response");
    assert_eq!(deserialized, resp);
}

#[test]
fn test_c2_04_sse_progress_event_golden() {
    let event = VoiceModelProgressEvent {
        event: "progress".to_string(),
        data: ModelInstallState::Downloading {
            current_file: "asr/model.int8.onnx".to_string(),
            file_index: 3,
            total_files: 7,
            downloaded_bytes: 50_000_000,
            total_bytes: 200_000_000,
            progress_percent: 25.0,
            speed_bytes_per_sec: 4_194_304,
        },
    };

    let expected_json = json!({
        "event": "progress",
        "data": {
            "status": "Downloading",
            "detail": {
                "current_file": "asr/model.int8.onnx",
                "file_index": 3,
                "total_files": 7,
                "downloaded_bytes": 50000000,
                "total_bytes": 200000000,
                "progress_percent": 25.0,
                "speed_bytes_per_sec": 4194304
            }
        }
    });

    let value = serde_json::to_value(&event).expect("serialize sse progress event");
    assert_eq!(value, expected_json);

    let deserialized: VoiceModelProgressEvent =
        serde_json::from_value(expected_json).expect("deserialize sse progress event");
    assert_eq!(deserialized, event);
}

#[test]
fn test_c2_05_delete_request_and_response_golden() {
    let req = VoiceModelDeleteRequest { force: false };
    let expected_req_json = json!({
        "force": false
    });
    let req_val = serde_json::to_value(&req).expect("serialize delete request");
    assert_eq!(req_val, expected_req_json);

    let resp = VoiceModelDeleteResponse {
        success: true,
        freed_bytes: 261_095_424,
        state: ModelInstallState::NotInstalled,
        message: Some("Models successfully removed".to_string()),
    };

    let expected_resp_json = json!({
        "success": true,
        "freed_bytes": 261095424,
        "state": {
            "status": "NotInstalled"
        },
        "message": "Models successfully removed"
    });

    let resp_val = serde_json::to_value(&resp).expect("serialize delete response");
    assert_eq!(resp_val, expected_resp_json);

    let deserialized: VoiceModelDeleteResponse =
        serde_json::from_value(expected_resp_json).expect("deserialize delete response");
    assert_eq!(deserialized, resp);
}


#[test]
fn shared_decision_parser_accepts_fenced_json_and_rejects_invalid_wire_fields() {
    use nomifun_api_types::voice::{parse_voice_decision,VoiceIntentType};
    let raw="```json\n{\"intent_type\":\"agent_task\",\"task_goal\":\"读取工作区文件\",\"requires_confirmation\":false}\n```";
    let decision=parse_voice_decision(raw).unwrap();
    assert_eq!(decision.intent_type,VoiceIntentType::AgentTask);
    assert_eq!(decision.task_goal.as_deref(),Some("读取工作区文件"));
    for raw in [r#"{"intent_type":"unknown"}"#,r#"{"intent_type":"chat"}"#,r#"{"intent_type":"chat","reply_tts":null}"#,r#"{"intent_type":"chat","reply_tts":"hello","requires_confirmation":"false"}"#,r#"{"intent_type":"agent_task","task_goal":123}"#,r#"{"intent_type":"ignore","understood_request":""}"#] {
        assert!(parse_voice_decision(raw).is_err(),"invalid shared decision: {raw}");
    }
}

#[test]
fn a_validated_chat_sentence_is_available_before_final_output_and_cannot_turn_into_a_task() {
    use nomifun_api_types::voice::VoiceDecisionStream;
    let mut stream=VoiceDecisionStream::default();
    assert!(stream.push(r#"{"intent_type":"chat","understood_request":"问候","reply_tts":"你好。""#).unwrap().is_empty());
    let first=stream.push("}\n").unwrap();
    assert_eq!(first.len(),1);assert_eq!(first[0].text,"你好。");
    let next=stream.push("{\"reply_delta\":\"我在听。\"}\n").unwrap();
    assert_eq!(next[0].text,"我在听。");
    assert_eq!(stream.finish().unwrap().reply_tts.as_deref(),Some("你好。我在听。"));
    let mut task=VoiceDecisionStream::default();
    assert!(task.push("{\"intent_type\":\"agent_task\",\"task_goal\":\"读取文件\",\"spoken_ack\":\"已经接单\"}\n").unwrap().is_empty());
    assert!(task.push("{\"reply_delta\":\"已经执行\"}\n").is_err());
    let mut invalid=VoiceDecisionStream::default();
    assert!(invalid.push("{\"intent_type\":\"chat\",\"reply_tts\":null}\n").is_err());
}
