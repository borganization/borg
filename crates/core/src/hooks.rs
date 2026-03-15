use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPoint {
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

            debug!("Dispatching {:?} to hook '{}'", ctx.point, hook.name());
            match hook.execute(ctx) {
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
                    debug!("Hook '{}' returned Skip for {:?}", hook.name(), ctx.point);
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
}
