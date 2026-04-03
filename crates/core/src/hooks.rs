use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPoint {
    /// Fired once when a new session begins (before any turns).
    SessionStart,
    /// Fired when a session ends (graceful shutdown or explicit close).
    SessionEnd,
    BeforeAgentStart,
    BeforeLlmCall,
    AfterLlmResponse,
    BeforeToolCall,
    AfterToolCall,
    TurnComplete,
    OnError,
}

#[derive(Debug, Clone)]
pub struct HookContext {
    pub point: HookPoint,
    pub session_id: String,
    pub turn_count: u32,
    pub data: HookData,
}

#[derive(Debug, Clone)]
pub enum HookData {
    SessionStart {
        session_id: String,
    },
    SessionEnd {
        session_id: String,
        total_turns: u32,
    },
    AgentStart {
        user_message: String,
    },
    LlmCall {
        message_count: usize,
    },
    LlmResponse {
        has_tool_calls: bool,
        text_length: usize,
    },
    ToolCall {
        name: String,
        args: String,
    },
    ToolResult {
        name: String,
        result: String,
        is_error: bool,
    },
    TurnEnd {
        total_tool_calls: u32,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub enum HookAction {
    Continue,
    InjectContext(String),
    Skip,
}

pub trait Hook: Send + Sync {
    fn name(&self) -> &str;
    fn points(&self) -> &[HookPoint];
    fn execute(&self, ctx: &HookContext) -> HookAction;
}

pub struct HookRegistry {
    hooks: Vec<Box<dyn Hook>>,
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn register(&mut self, hook: Box<dyn Hook>) {
        debug!("Registered hook: {}", hook.name());
        self.hooks.push(hook);
    }

    pub fn dispatch(&self, ctx: &HookContext) -> HookAction {
        let mut result = HookAction::Continue;

        for hook in &self.hooks {
            if !hook.points().contains(&ctx.point) {
                continue;
            }

            let hook_name = hook.name().to_string();
            debug!("Dispatching {:?} to hook '{}'", ctx.point, hook_name);

            let start = Instant::now();
            let outcome = catch_unwind(AssertUnwindSafe(|| hook.execute(ctx)));
            let elapsed = start.elapsed();

            if elapsed.as_secs() >= 5 {
                warn!(
                    "Hook '{}' took {:.1}s for {:?} — consider optimizing",
                    hook_name,
                    elapsed.as_secs_f64(),
                    ctx.point,
                );
            }

            let action = match outcome {
                Ok(action) => action,
                Err(_) => {
                    warn!(
                        "Hook '{}' panicked during {:?}, skipping",
                        hook_name, ctx.point,
                    );
                    continue;
                }
            };

            match action {
                HookAction::Continue => {}
                HookAction::InjectContext(text) => {
                    result = match result {
                        HookAction::InjectContext(existing) => {
                            HookAction::InjectContext(format!("{existing}\n{text}"))
                        }
                        _ => HookAction::InjectContext(text),
                    };
                }
                HookAction::Skip => {
                    debug!("Hook '{}' returned Skip for {:?}", hook_name, ctx.point);
                    return HookAction::Skip;
                }
            }
        }

        result
    }

    pub fn hook_count(&self) -> usize {
        self.hooks.len()
    }
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRegistry")
            .field("hook_count", &self.hooks.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHook {
        name: String,
        points: Vec<HookPoint>,
        action: HookAction,
    }

    impl Hook for TestHook {
        fn name(&self) -> &str {
            &self.name
        }
        fn points(&self) -> &[HookPoint] {
            &self.points
        }
        fn execute(&self, _ctx: &HookContext) -> HookAction {
            self.action.clone()
        }
    }

    fn make_ctx(point: HookPoint) -> HookContext {
        HookContext {
            point,
            session_id: "test-session".to_string(),
            turn_count: 1,
            data: HookData::AgentStart {
                user_message: "hello".to_string(),
            },
        }
    }

    #[test]
    fn empty_registry_returns_continue() {
        let registry = HookRegistry::new();
        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        assert!(matches!(registry.dispatch(&ctx), HookAction::Continue));
    }

    #[test]
    fn hook_receives_matching_point() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "test".to_string(),
            points: vec![HookPoint::BeforeAgentStart],
            action: HookAction::InjectContext("injected".to_string()),
        }));

        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        match registry.dispatch(&ctx) {
            HookAction::InjectContext(text) => assert_eq!(text, "injected"),
            other => panic!("Expected InjectContext, got {other:?}"),
        }
    }

    #[test]
    fn hook_ignores_non_matching_point() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "test".to_string(),
            points: vec![HookPoint::TurnComplete],
            action: HookAction::InjectContext("injected".to_string()),
        }));

        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        assert!(matches!(registry.dispatch(&ctx), HookAction::Continue));
    }

    #[test]
    fn skip_short_circuits() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "skipper".to_string(),
            points: vec![HookPoint::BeforeToolCall],
            action: HookAction::Skip,
        }));
        registry.register(Box::new(TestHook {
            name: "injector".to_string(),
            points: vec![HookPoint::BeforeToolCall],
            action: HookAction::InjectContext("should not reach".to_string()),
        }));

        let ctx = HookContext {
            point: HookPoint::BeforeToolCall,
            session_id: "test".to_string(),
            turn_count: 1,
            data: HookData::ToolCall {
                name: "test_tool".to_string(),
                args: "{}".to_string(),
            },
        };
        assert!(matches!(registry.dispatch(&ctx), HookAction::Skip));
    }

    #[test]
    fn multiple_inject_contexts_merge() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "hook1".to_string(),
            points: vec![HookPoint::BeforeAgentStart],
            action: HookAction::InjectContext("first".to_string()),
        }));
        registry.register(Box::new(TestHook {
            name: "hook2".to_string(),
            points: vec![HookPoint::BeforeAgentStart],
            action: HookAction::InjectContext("second".to_string()),
        }));

        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        match registry.dispatch(&ctx) {
            HookAction::InjectContext(text) => {
                assert!(text.contains("first"));
                assert!(text.contains("second"));
            }
            other => panic!("Expected InjectContext, got {other:?}"),
        }
    }

    #[test]
    fn hook_count() {
        let mut registry = HookRegistry::new();
        assert_eq!(registry.hook_count(), 0);
        registry.register(Box::new(TestHook {
            name: "a".to_string(),
            points: vec![HookPoint::BeforeAgentStart],
            action: HookAction::Continue,
        }));
        assert_eq!(registry.hook_count(), 1);
    }

    #[test]
    fn continue_hook_does_not_affect_result() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "noop".to_string(),
            points: vec![HookPoint::BeforeLlmCall],
            action: HookAction::Continue,
        }));
        let ctx = HookContext {
            point: HookPoint::BeforeLlmCall,
            session_id: "test".to_string(),
            turn_count: 1,
            data: HookData::LlmCall { message_count: 5 },
        };
        assert!(matches!(registry.dispatch(&ctx), HookAction::Continue));
    }

    #[test]
    fn hook_multi_point_registration() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "multi".to_string(),
            points: vec![HookPoint::BeforeAgentStart, HookPoint::TurnComplete],
            action: HookAction::InjectContext("ctx".to_string()),
        }));

        let ctx1 = make_ctx(HookPoint::BeforeAgentStart);
        assert!(matches!(
            registry.dispatch(&ctx1),
            HookAction::InjectContext(_)
        ));

        let ctx2 = HookContext {
            point: HookPoint::TurnComplete,
            session_id: "test".to_string(),
            turn_count: 1,
            data: HookData::TurnEnd {
                total_tool_calls: 3,
            },
        };
        assert!(matches!(
            registry.dispatch(&ctx2),
            HookAction::InjectContext(_)
        ));
    }

    #[test]
    fn skip_before_inject_short_circuits() {
        let mut registry = HookRegistry::new();
        // Skip registered first
        registry.register(Box::new(TestHook {
            name: "skipper".to_string(),
            points: vec![HookPoint::AfterLlmResponse],
            action: HookAction::Skip,
        }));
        // Inject registered second — should not execute
        registry.register(Box::new(TestHook {
            name: "injector".to_string(),
            points: vec![HookPoint::AfterLlmResponse],
            action: HookAction::InjectContext("should not appear".to_string()),
        }));

        let ctx = HookContext {
            point: HookPoint::AfterLlmResponse,
            session_id: "test".to_string(),
            turn_count: 1,
            data: HookData::LlmResponse {
                has_tool_calls: false,
                text_length: 100,
            },
        };
        assert!(matches!(registry.dispatch(&ctx), HookAction::Skip));
    }

    #[test]
    fn registry_debug_format() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "test".to_string(),
            points: vec![HookPoint::BeforeAgentStart],
            action: HookAction::Continue,
        }));
        let debug = format!("{registry:?}");
        assert!(debug.contains("hook_count: 1"));
    }

    #[test]
    fn hook_data_variants_constructible() {
        // Ensure all HookData variants can be constructed (compile-time coverage)
        let _ = HookData::AgentStart {
            user_message: "hi".to_string(),
        };
        let _ = HookData::LlmCall { message_count: 1 };
        let _ = HookData::LlmResponse {
            has_tool_calls: true,
            text_length: 42,
        };
        let _ = HookData::ToolCall {
            name: "foo".to_string(),
            args: "{}".to_string(),
        };
        let _ = HookData::ToolResult {
            name: "foo".to_string(),
            result: "ok".to_string(),
            is_error: false,
        };
        let _ = HookData::TurnEnd {
            total_tool_calls: 5,
        };
        let _ = HookData::Error {
            message: "oops".to_string(),
        };
    }

    #[test]
    fn hook_point_equality() {
        assert_eq!(HookPoint::BeforeAgentStart, HookPoint::BeforeAgentStart);
        assert_ne!(HookPoint::BeforeAgentStart, HookPoint::TurnComplete);
    }

    #[test]
    fn session_start_hook_fires() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "session_start".to_string(),
            points: vec![HookPoint::SessionStart],
            action: HookAction::InjectContext("session started".to_string()),
        }));

        let ctx = HookContext {
            point: HookPoint::SessionStart,
            session_id: "s1".to_string(),
            turn_count: 0,
            data: HookData::SessionStart {
                session_id: "s1".to_string(),
            },
        };
        match registry.dispatch(&ctx) {
            HookAction::InjectContext(text) => assert_eq!(text, "session started"),
            other => panic!("Expected InjectContext, got {other:?}"),
        }
    }

    #[test]
    fn session_end_hook_fires() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "session_end".to_string(),
            points: vec![HookPoint::SessionEnd],
            action: HookAction::InjectContext("session ended".to_string()),
        }));

        let ctx = HookContext {
            point: HookPoint::SessionEnd,
            session_id: "s1".to_string(),
            turn_count: 5,
            data: HookData::SessionEnd {
                session_id: "s1".to_string(),
                total_turns: 5,
            },
        };
        match registry.dispatch(&ctx) {
            HookAction::InjectContext(text) => assert_eq!(text, "session ended"),
            other => panic!("Expected InjectContext, got {other:?}"),
        }
    }

    #[test]
    fn session_hooks_do_not_fire_on_other_points() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(TestHook {
            name: "session_only".to_string(),
            points: vec![HookPoint::SessionStart, HookPoint::SessionEnd],
            action: HookAction::InjectContext("session".to_string()),
        }));

        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        assert!(matches!(registry.dispatch(&ctx), HookAction::Continue));
    }

    #[test]
    fn hook_data_session_variants_constructible() {
        let _ = HookData::SessionStart {
            session_id: "s1".to_string(),
        };
        let _ = HookData::SessionEnd {
            session_id: "s1".to_string(),
            total_turns: 10,
        };
    }

    /// A hook that panics on execute — used to verify dispatch resilience.
    struct PanicHook;

    impl Hook for PanicHook {
        fn name(&self) -> &str {
            "panicker"
        }
        fn points(&self) -> &[HookPoint] {
            &[HookPoint::BeforeAgentStart]
        }
        fn execute(&self, _ctx: &HookContext) -> HookAction {
            panic!("intentional panic in hook");
        }
    }

    #[test]
    fn panicking_hook_does_not_crash_dispatch() {
        let mut registry = HookRegistry::new();
        registry.register(Box::new(PanicHook));

        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        // Should return Continue, not propagate panic
        assert!(matches!(registry.dispatch(&ctx), HookAction::Continue));
    }

    #[test]
    fn panicking_hook_does_not_block_subsequent_hooks() {
        let mut registry = HookRegistry::new();
        // Panicking hook registered first
        registry.register(Box::new(PanicHook));
        // Normal hook registered second — should still execute
        registry.register(Box::new(TestHook {
            name: "survivor".to_string(),
            points: vec![HookPoint::BeforeAgentStart],
            action: HookAction::InjectContext("survived".to_string()),
        }));

        let ctx = make_ctx(HookPoint::BeforeAgentStart);
        match registry.dispatch(&ctx) {
            HookAction::InjectContext(text) => assert_eq!(text, "survived"),
            other => panic!("Expected InjectContext, got {other:?}"),
        }
    }
}
