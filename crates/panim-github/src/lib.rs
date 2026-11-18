//! The GitHub connector of PANIM. Refer to `docs/ARCHITECTURE.md` §4a.
//!
//! This crate has the network code of the connector. `panim-core` has no network code.
//!
//! - `http` has the `Http` interface and the real client. The client uses the route of the persona.
//! - `login` does the OAuth device flow.
//! - `sync` reads the notifications, the watched repositories and the received events, and makes timeline items
//!   from them. It keeps only the events of the watched repositories.
//! - `answer` posts a comment on an issue or a pull request.
//! - `gist` examines an account proof in a public gist, without a token.
//!
//! All the functions use the `Http` interface. Thus, the tests use a fake client and no network.

pub mod answer;
pub mod api;
pub mod gist;
pub mod http;
pub mod login;
pub mod sync;

pub use answer::{Target, answer_target, post_comment};
pub use api::Error;
pub use gist::{BlockView, GistCheck, GistResult, Owner, Verdict, check_gist, gist_id};
pub use http::{Client, Http, HttpError, Method, Request, Response};
pub use login::{CLIENT_ID, DeviceLogin, Poll, SCOPES, User, poll_login, start_login, user};
pub use sync::{
    Fetched, Watched, event_kinds, event_repo, event_summary, has_read_state, is_watched_event, mark_thread_read,
    notifications, received_events, watched,
};
