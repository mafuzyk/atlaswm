use std::collections::HashMap;
use std::any::Any;

pub struct EventBus {
    handlers: HashMap<&'static str, Vec<Box<dyn FnMut(&dyn Any)>>>,
}

impl EventBus {
    pub fn new() -> Self {
        EventBus {
            handlers: HashMap::new(),
        }
    }

    pub fn on<F, E>(&mut self, _event_type: &str, _handler: F)
    where
        F: FnMut(&E) + 'static,
        E: 'static,
    {
    }

    pub fn emit<E: 'static>(&mut self, _event: &E) {
    }
}
