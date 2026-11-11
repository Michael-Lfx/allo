//! Subtractive policy for voice-owned tasks. Approval remains in ToolApprovalManager.
use std::{path::{Path, PathBuf}, time::Duration};
use nomi_agent::tool_execution::{InvocationDecision, ToolInvocationPolicy, is_path_within_invocation_root, patch_invocation_paths};
use serde_json::Value;
use crate::tool_policy::{ToolPolicyTable, inspect_command_string, split_literal_command};

/// Complete action targets for an oral prompt; oversized scopes require the
/// full-argument panel instead of silently truncating an authorization.
pub fn voice_confirmation_summary(tool:&str,input:&Value)->Option<String> {
    let summary=match tool {
        "Write"|"Edit"=>format!("{}文件 {}",if tool=="Write"{"写入"}else{"修改"},input.get("file_path").or_else(||input.get("path"))?.as_str()?),
        "ApplyPatch"=>format!("修改或删除文件 {}",patch_invocation_paths(input).ok()?.join("，")),
        "Bash"|"exec_command"=>format!("执行命令 {}",input.get("command").or_else(||input.get("cmd"))?.as_str()?),
        "WebSearch"|"web_search"=> {
            if let Some(q) = input.get("query").or_else(|| input.get("q")).and_then(Value::as_str) {
                format!("搜索网络「{q}」")
            } else if let Some(desc) = input.get("description").and_then(Value::as_str) {
                let query = desc
                    .strip_prefix("web_search '")
                    .and_then(|s| s.strip_suffix('\''))
                    .unwrap_or(desc);
                format!("搜索网络「{query}」")
            } else {
                "搜索网络".into()
            }
        },
        "WebFetch"|"web_fetch"=> {
            if let Some(url) = input.get("url").and_then(Value::as_str) {
                format!("访问网页 {url}")
            } else if let Some(desc) = input.get("description").and_then(Value::as_str) {
                desc.to_string()
            } else {
                "访问网页".into()
            }
        },
        _ => {
            if let Some(desc) = input.get("description").and_then(Value::as_str) {
                if !desc.trim().is_empty() {
                    desc.to_string()
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    };
    (summary.chars().count()<=240).then_some(summary)
}

pub(crate) struct VoiceInvocationPolicy {
    root: PathBuf,
    table: ToolPolicyTable,
    auto_approve: bool,
}
impl VoiceInvocationPolicy {
    #[allow(dead_code)]
    pub fn new(root: PathBuf) -> Result<Self, String> {
        Self::with_auto_approve(root, false)
    }

    pub fn with_auto_approve(root: PathBuf, auto_approve: bool) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|_| "voice_workspace_unavailable")?;
        if !root.is_dir() {
            return Err("voice_workspace_unavailable".into());
        }
        let table = ToolPolicyTable {
            sandbox_root: root.clone(),
            ..Default::default()
        };
        Ok(Self {
            root,
            table,
            auto_approve,
        })
    }
    fn deny(code:&str)->InvocationDecision {InvocationDecision::Deny(code.into())}
    fn confirmation(reason:&str)->InvocationDecision {InvocationDecision::RequireApproval {reason:reason.into(),timeout:Duration::from_secs(45)}}
    fn within(&self,path:&str,cwd:&Path)->bool {
        if path.is_empty() {return false;}
        let path=Path::new(path);let resolved=if path.is_absolute(){path.to_path_buf()}else{cwd.join(path)};
        is_path_within_invocation_root(&resolved,&self.root)
    }
    fn shell(&self,input:&Value)->InvocationDecision {
        if input.get("script").is_some() {return Self::deny("voice_command_unsupported");}
        let Some(command)=input.get("command").or_else(||input.get("cmd")).and_then(Value::as_str) else{return Self::deny("voice_command_invalid");};
        let Some(words)=split_literal_command(command) else{return Self::deny("voice_command_unsupported");};
        let cwd=input.get("workdir").or_else(||input.get("cwd")).and_then(Value::as_str).map(PathBuf::from).unwrap_or_else(||self.root.clone());
        if !self.within(&cwd.to_string_lossy(),&self.root) {return Self::deny("voice_workspace_escape");}
        let executable=words[0].to_ascii_lowercase();
        let destructive=matches!(executable.as_str(),"rm"|"rmdir"|"unlink"|"remove-item"|"del"|"erase");
        if matches!(executable.as_str(),"pwd"|"date"|"whoami"|"get-location"|"get-date") {
            return if words.len()==1 {InvocationDecision::Allow}else{Self::deny("voice_command_unsupported")};
        }
        if !destructive && !matches!(executable.as_str(),"ls"|"cat"|"head"|"tail"|"wc"|"stat"|"get-content"|"get-childitem"|"get-item"|"test-path") {return Self::deny("voice_command_unsupported");}
        let mut paths=0;
        for argument in &words[1..] {
            if argument.starts_with('-') {
                if !matches!(argument.to_ascii_lowercase().as_str(),"-r"|"-f"|"-rf"|"-fr"|"-a"|"-l"|"-la"|"-al"|"-n"|"-recurse"|"-force"|"-literalpath"|"-path"|"--") {return Self::deny("voice_command_unsupported");}
            } else {
                paths+=1;
                if !self.within(argument,&cwd) {return Self::deny("voice_workspace_escape");}
            }
        }
        if destructive && paths==0 {return Self::deny("voice_command_invalid");}
        if destructive || inspect_command_string(command,&self.table.blocked_commands).is_some() {Self::confirmation("voice_destructive_command")}else{InvocationDecision::Allow}
    }
}

impl ToolInvocationPolicy for VoiceInvocationPolicy {
    fn evaluate(&self,name:&str,input:&Value)->InvocationDecision {
        if self.root.canonicalize().ok().as_ref()!=Some(&self.root) {return Self::deny("voice_workspace_changed");}
        match name {
            "Bash"|"exec_command" => self.shell(input),
            "Read"|"Write"|"Edit"|"Glob"|"Grep" => {
                let path=input.get("file_path").or_else(||input.get("path")).and_then(Value::as_str).unwrap_or(".");
                if !self.within(path,&self.root) {return Self::deny("voice_workspace_escape");}
                if name=="Glob" {
                    let base=if Path::new(path).is_absolute(){PathBuf::from(path)}else{self.root.join(path)};
                    if let Some(pattern)=input.get("pattern").and_then(Value::as_str) {
                        if !self.within(pattern,&base) {return Self::deny("voice_workspace_escape");}
                    }
                }
                if matches!(name, "Write" | "Edit") {
                    if self.auto_approve {
                        InvocationDecision::Allow
                    } else {
                        Self::confirmation("voice_file_change")
                    }
                } else {
                    InvocationDecision::Allow
                }
            }
            "ApplyPatch" => {
                let Ok(paths) = patch_invocation_paths(input) else {
                    return Self::deny("voice_patch_invalid");
                };
                if paths.is_empty() || paths.iter().any(|path| !self.within(path, &self.root)) {
                    return Self::deny("voice_workspace_escape");
                }
                if self.auto_approve {
                    InvocationDecision::Allow
                } else {
                    Self::confirmation("voice_file_change")
                }
            }
            "ToolSearch"|"update_plan"|"WebSearch"|"WebFetch"|"web_search"|"web_fetch"|"web_extract"|"WebExtract" => InvocationDecision::Allow,
            _ => Self::deny("voice_tool_unsupported"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicUsize,Ordering}};
    use nomi_protocol::{ToolApprovalManager, events::ProtocolEvent, writer::ProtocolEmitter};
    use nomi_types::message::ContentBlock;
    use nomi_tools::registry::ToolRegistry;
    use serde_json::json;

    #[test]
    fn oral_summaries_include_all_patch_targets_or_require_the_full_panel() {
        let summary=voice_confirmation_summary("ApplyPatch",&json!({"patch":"*** Begin Patch\n*** Delete File: first.txt\n*** Delete File: second.txt\n*** End Patch"})).unwrap();
        assert!(summary.contains("first.txt")&&summary.contains("second.txt"));
        assert!(voice_confirmation_summary("Write",&json!({"file_path":"x".repeat(241),"content":"text"})).is_none());
        assert!(voice_confirmation_summary("unknown",&json!({})).is_none());

        // Web search with query
        let ws = voice_confirmation_summary("web_search", &json!({"query": "杭州明天天气"})).unwrap();
        assert_eq!(ws, "搜索网络「杭州明天天气」");

        // Web search with description fallback
        let ws_desc = voice_confirmation_summary("web_search", &json!({"description": "web_search '杭州明天天气'"})).unwrap();
        assert_eq!(ws_desc, "搜索网络「杭州明天天气」");

        // General tool with description
        let general = voice_confirmation_summary("mcp_custom_tool", &json!({"description": "获取用户订单列表"})).unwrap();
        assert_eq!(general, "获取用户订单列表");
    }

    #[test]
    fn production_voice_writes_require_approval_and_unknown_tools_are_denied() {
        let dir = tempfile::tempdir().unwrap();
        let policy = VoiceInvocationPolicy::new(dir.path().to_path_buf()).unwrap();
        assert!(matches!(policy.evaluate("Read", &json!({"file_path": "owned.txt"})), InvocationDecision::Allow));
        for name in ["Write", "Edit"] {
            assert!(matches!(policy.evaluate(name, &json!({"file_path": "owned.txt"})), InvocationDecision::RequireApproval {..}));
        }
        assert!(matches!(policy.evaluate("mcp__unknown__delete",&json!({})),InvocationDecision::Deny(_)));
    }

    #[test]
    fn paths_commands_and_unknown_tools_are_checked_independently_of_model_flags() {
        let dir=tempfile::tempdir().unwrap();let outside=tempfile::tempdir().unwrap();let policy=VoiceInvocationPolicy::new(dir.path().to_path_buf()).unwrap();
        assert!(matches!(policy.evaluate("Write",&json!({"file_path":"inside.txt","requires_confirmation":false})),InvocationDecision::RequireApproval {..}));
        for name in ["Read","Write","Edit","Grep"] {
            assert!(matches!(policy.evaluate(name,&json!({"file_path":outside.path().join("outside.txt")})),InvocationDecision::Deny(_)));
        }
        assert!(matches!(policy.evaluate("Glob",&json!({"pattern":"../*"})),InvocationDecision::Deny(_)));
        assert!(matches!(policy.evaluate("Glob",&json!({"pattern":"**/*.rs"})),InvocationDecision::Allow));
        assert!(matches!(policy.evaluate("ApplyPatch",&json!({"patch":format!("*** Begin Patch\n*** Update File: inside.txt\n*** Move to: {}\n@@\n-a\n+b\n*** End Patch",outside.path().join("outside.txt").display())})),InvocationDecision::Deny(_)));
        assert!(matches!(policy.evaluate("Bash",&json!({"command":"Remove-Item -LiteralPath 'inside.txt'"})),InvocationDecision::RequireApproval {..}));
        assert!(matches!(policy.evaluate("Bash",&json!({"command":format!("Remove-Item -LiteralPath '{}'",outside.path().join("outside.txt").display())})),InvocationDecision::Deny(_)));
        for command in ["ls; Remove-Item file", "cat $HOME/file", "python script.py", "rm *.txt", "ls 'unterminated"] {assert!(matches!(policy.evaluate("Bash",&json!({"command":command})),InvocationDecision::Deny(_)),"{command}");}
        assert!(matches!(policy.evaluate("mcp__unknown__delete",&json!({})),InvocationDecision::Deny(_)));
        assert!(matches!(policy.evaluate("WebSearch",&json!({"query":"weather"})),InvocationDecision::Allow));
    }

    #[derive(Default)]
    struct Emitter(AtomicUsize);
    impl ProtocolEmitter for Emitter {
        fn emit(&self,event:&ProtocolEvent)->std::io::Result<()> {
            if matches!(event,ProtocolEvent::ToolRequest {..}) {self.0.fetch_add(1,Ordering::SeqCst);}
            Ok(())
        }
    }

    #[tokio::test]
    async fn actual_patch_delete_requires_the_bound_approval_and_respects_denial() {
        for approved in [false,true] {
            let dir=tempfile::tempdir().unwrap();let file=dir.path().join("owned.txt");std::fs::write(&file,"owned test data").unwrap();
            let mut registry=ToolRegistry::new();registry.register(Box::new(nomi_tools::apply_patch::ApplyPatchTool::new(None).with_cwd(Some(dir.path().to_path_buf()))));
            registry.set_invocation_policy(Arc::new(VoiceInvocationPolicy::new(dir.path().to_path_buf()).unwrap()));
            let calls=[ContentBlock::ToolUse {id:"delete-call".into(),name:"ApplyPatch".into(),input:json!({"patch":"*** Begin Patch\n*** Delete File: owned.txt\n*** End Patch"}),extra:None}];
            let manager=Arc::new(ToolApprovalManager::new());manager.set_mode(nomi_protocol::commands::SessionMode::Yolo);
            let resolver=manager.clone();let target=file.clone();
            let resolution=tokio::spawn(async move {
                loop { if let Some(scope)=resolver.pending_scoped_approvals().into_iter().next() {
                    assert!(target.exists(),"the real deletion cannot precede the approval");
                    assert_eq!(scope.msg_id,"actual-message");assert_eq!(scope.call_id,"delete-call");
                    assert!(resolver.resolve_scoped_once(&scope.msg_id,&scope.call_id,&scope.approval_id,approved));break;
                } tokio::time::sleep(Duration::from_millis(2)).await; }
            });
            let captured=Arc::new(Emitter::default());let writer:Arc<dyn ProtocolEmitter>=captured.clone();
            let outcome=nomi_agent::tool_execution::execute_tool_calls_with_approval(&registry,&calls,&nomi_agent::tool_execution::ProviderToolAuthority::from_request_tools(&registry.to_tool_defs()),&manager,&writer,"actual-message",true,&["ApplyPatch".into()],None,Default::default(),false,Default::default()).await.unwrap();
            resolution.abort();assert_eq!(captured.0.load(Ordering::SeqCst),1);assert_eq!(file.exists(),!approved);
            assert!(matches!(&outcome.results[0],ContentBlock::ToolResult {is_error,..} if *is_error==!approved));
        }
    }

    #[tokio::test]
    async fn an_actual_outside_delete_is_refused_before_prompt_or_execution() {
        let root=tempfile::tempdir().unwrap();let outside=tempfile::tempdir().unwrap();let file=outside.path().join("keep.txt");std::fs::write(&file,"keep").unwrap();
        let mut registry=ToolRegistry::new();registry.register(Box::new(nomi_tools::apply_patch::ApplyPatchTool::new(None)));
        registry.set_invocation_policy(Arc::new(VoiceInvocationPolicy::new(root.path().to_path_buf()).unwrap()));
        let calls=[ContentBlock::ToolUse {id:"outside-delete".into(),name:"ApplyPatch".into(),input:json!({"files":[{"file_path":file,"delete":true}]}),extra:None}];
        let manager=Arc::new(ToolApprovalManager::new());let captured=Arc::new(Emitter::default());let writer:Arc<dyn ProtocolEmitter>=captured.clone();
        let outcome=nomi_agent::tool_execution::execute_tool_calls_with_approval(&registry,&calls,&nomi_agent::tool_execution::ProviderToolAuthority::from_request_tools(&registry.to_tool_defs()),&manager,&writer,"message",true,&["ApplyPatch".into()],None,Default::default(),false,Default::default()).await.unwrap();
        assert_eq!(std::fs::read_to_string(file).unwrap(),"keep");assert_eq!(captured.0.load(Ordering::SeqCst),0);
        assert!(matches!(&outcome.results[0],ContentBlock::ToolResult {is_error:true,..}));
    }

    #[tokio::test]
    async fn an_expired_approval_cannot_resume_an_actual_file_deletion() {
        struct ShortDeadline(VoiceInvocationPolicy);
        impl ToolInvocationPolicy for ShortDeadline {
            fn evaluate(&self,name:&str,input:&Value)->InvocationDecision {
                match self.0.evaluate(name,input) {
                    InvocationDecision::RequireApproval {reason,..}=>InvocationDecision::RequireApproval {reason,timeout:Duration::from_millis(40)},
                    other=>other,
                }
            }
        }
        let root=tempfile::tempdir().unwrap();let path=root.path().join("keep.txt");std::fs::write(&path,"keep").unwrap();
        let mut registry=ToolRegistry::new();registry.register(Box::new(nomi_tools::apply_patch::ApplyPatchTool::new(None).with_cwd(Some(root.path().to_path_buf()))));
        registry.set_invocation_policy(Arc::new(ShortDeadline(VoiceInvocationPolicy::new(root.path().to_path_buf()).unwrap())));
        let manager=Arc::new(ToolApprovalManager::new());manager.set_mode(nomi_protocol::commands::SessionMode::Yolo);
        let resolver=manager.clone();let preserved=path.clone();
        let late=async move {
            let scope=loop {if let Some(scope)=resolver.pending_scoped_approvals().into_iter().next(){break scope;}tokio::time::sleep(Duration::from_millis(1)).await;};
            assert!(preserved.exists());tokio::time::sleep(Duration::from_millis(80)).await;
            assert!(!resolver.resolve_scoped_once(&scope.msg_id,&scope.call_id,&scope.approval_id,true));
        };
        let calls=[ContentBlock::ToolUse {id:"delete-call".into(),name:"ApplyPatch".into(),input:json!({"patch":"*** Begin Patch\n*** Delete File: keep.txt\n*** End Patch"}),extra:None}];
        let captured=Arc::new(Emitter::default());let writer:Arc<dyn ProtocolEmitter>=captured;
        let authority=nomi_agent::tool_execution::ProviderToolAuthority::from_request_tools(&registry.to_tool_defs());
        let allowed=vec!["ApplyPatch".into()];
        let (result,_)=tokio::join!(nomi_agent::tool_execution::execute_tool_calls_with_approval(&registry,&calls,&authority,&manager,&writer,"actual-message",true,&allowed,None,Default::default(),false,Default::default()),late);
        assert_eq!(std::fs::read_to_string(path).unwrap(),"keep");
        assert!(matches!(&result.unwrap().results[0],ContentBlock::ToolResult {is_error:true,..}));
    }

    #[test]
    fn test_web_extract_and_yolo_mode_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        // 1. Default policy: web_extract is allowed without approval
        let default_policy = VoiceInvocationPolicy::new(dir.path().to_path_buf()).unwrap();
        assert!(matches!(
            default_policy.evaluate("web_extract", &json!({"url": "https://example.com"})),
            InvocationDecision::Allow
        ));
        assert!(matches!(
            default_policy.evaluate("WebExtract", &json!({"url": "https://example.com"})),
            InvocationDecision::Allow
        ));

        // 2. YOLO mode: in-workspace writes evaluate to Allow
        let yolo_policy = VoiceInvocationPolicy::with_auto_approve(dir.path().to_path_buf(), true).unwrap();
        assert!(matches!(
            yolo_policy.evaluate("Write", &json!({"file_path": "inside.txt"})),
            InvocationDecision::Allow
        ));
        assert!(matches!(
            yolo_policy.evaluate("Edit", &json!({"file_path": "inside.txt"})),
            InvocationDecision::Allow
        ));

        // 3. YOLO mode must STILL strictly block workspace escape and dangerous commands
        assert!(matches!(
            yolo_policy.evaluate("Write", &json!({"file_path": outside.path().join("outside.txt")})),
            InvocationDecision::Deny(_)
        ));
        assert!(matches!(
            yolo_policy.evaluate("Bash", &json!({"command": "rm -rf /"})),
            InvocationDecision::Deny(_)
        ));
        assert!(matches!(
            yolo_policy.evaluate("mcp__unknown__delete", &json!({})),
            InvocationDecision::Deny(_)
        ));
    }
}
