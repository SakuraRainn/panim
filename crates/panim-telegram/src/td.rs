//! The interface to TDLib, with the real library and a fake for the tests.
//!
//! All the logic of the connector uses the `Td` interface. Thus, the tests give scripted updates and examine the
//! requests, and they use no network and no Telegram account.

use std::time::Duration;

use serde_json::Value;

use crate::tdjson;

/// The four functions of the JSON interface of TDLib.
pub trait Td: Send + Sync {
    /// Runs a synchronous request, for example a log setting.
    fn execute(&self, request: &Value) -> Option<Value>;
    /// Makes the id of a new client. TDLib starts the client when it gets its first request.
    fn create_client(&self) -> i32;
    /// Sends a request to a client. The answer comes through `receive` with the same `@extra` field.
    fn send(&self, client: i32, request: &Value);
    /// Waits for the next answer or update of all the clients. Only one thread can call this function.
    fn receive(&self, timeout: Duration) -> Option<Value>;
}

/// The real TDLib (`libtdjson.so`).
pub struct RealTd;

impl Td for RealTd {
    fn execute(&self, request: &Value) -> Option<Value> {
        tdjson::execute(request)
    }

    fn create_client(&self) -> i32 {
        tdjson::create_client_id()
    }

    fn send(&self, client: i32, request: &Value) {
        tdjson::send(client, request)
    }

    fn receive(&self, timeout: Duration) -> Option<Value> {
        tdjson::receive(timeout.as_secs_f64())
    }
}

/// A fake TDLib for tests. It records each request and gives the scripted answers and updates.
#[cfg(any(test, feature = "fake"))]
pub mod fake {
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    use serde_json::Value;

    use super::Td;

    /// A function that makes the answers and the updates for one request.
    pub type Responder = Box<dyn FnMut(i32, &Value) -> Vec<Value> + Send>;

    #[derive(Default)]
    struct State {
        clients: i32,
        executed: Vec<Value>,
        sent: Vec<(i32, Value)>,
        incoming: VecDeque<Value>,
        responder: Option<Responder>,
        /// The number of all the calls, in their sequence. A test uses it to examine the order.
        log: Vec<String>,
    }

    #[derive(Default)]
    pub struct FakeTd {
        state: Mutex<State>,
        wake: Condvar,
    }

    impl FakeTd {
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the function that answers each request. The fake adds `@client_id` and, for an answer that has the
        /// field `@answer`, the `@extra` of the request.
        pub fn respond(&self, responder: impl FnMut(i32, &Value) -> Vec<Value> + Send + 'static) {
            self.state.lock().unwrap().responder = Some(Box::new(responder));
        }

        /// Adds an update or an answer to the queue of `receive`.
        pub fn push(&self, client: i32, mut value: Value) {
            value["@client_id"] = client.into();
            self.state.lock().unwrap().incoming.push_back(value);
            self.wake.notify_all();
        }

        /// The requests that the connector sent until now. The call removes them from the record.
        pub fn take_sent(&self) -> Vec<(i32, Value)> {
            std::mem::take(&mut self.state.lock().unwrap().sent)
        }

        /// The synchronous requests until now.
        pub fn executed(&self) -> Vec<Value> {
            self.state.lock().unwrap().executed.clone()
        }

        /// The names of all the calls, in their sequence: `execute <type>`, `create <id>`, `send <id> <type>`.
        pub fn log(&self) -> Vec<String> {
            self.state.lock().unwrap().log.clone()
        }
    }

    fn kind(value: &Value) -> &str {
        value["@type"].as_str().unwrap_or("?")
    }

    impl Td for FakeTd {
        fn execute(&self, request: &Value) -> Option<Value> {
            let mut state = self.state.lock().unwrap();
            state.log.push(format!("execute {}", kind(request)));
            state.executed.push(request.clone());
            Some(serde_json::json!({"@type": "ok"}))
        }

        fn create_client(&self) -> i32 {
            let mut state = self.state.lock().unwrap();
            state.clients += 1;
            let id = state.clients;
            state.log.push(format!("create {id}"));
            id
        }

        fn send(&self, client: i32, request: &Value) {
            let mut state = self.state.lock().unwrap();
            state.log.push(format!("send {client} {}", kind(request)));
            state.sent.push((client, request.clone()));
            let Some(mut responder) = state.responder.take() else {
                return;
            };
            let answers = responder(client, request);
            state.responder = Some(responder);
            for mut answer in answers {
                answer["@client_id"] = client.into();
                if let Some(object) = answer.as_object_mut()
                    && object.remove("@answer").is_some()
                {
                    object.insert("@extra".into(), request["@extra"].clone());
                }
                state.incoming.push_back(answer);
            }
            self.wake.notify_all();
        }

        fn receive(&self, timeout: Duration) -> Option<Value> {
            let mut state = self.state.lock().unwrap();
            if state.incoming.is_empty() {
                state = self.wake.wait_timeout(state, timeout).unwrap().0;
            }
            state.incoming.pop_front()
        }
    }
}
