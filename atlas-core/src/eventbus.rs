pub struct EventBus;

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        EventBus
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
