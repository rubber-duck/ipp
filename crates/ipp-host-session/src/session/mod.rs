//! One World session's request pipeline: ingress admission, per-request dispatch at the Host
//! mutation boundary, and replies published after evaluation, with the session-scoped receipts,
//! inspection, view queries, lifecycle watches and GUI observations they answer.

pub(crate) mod attachment_receipts;
pub(crate) mod gui_observations;
pub(crate) mod ingress;
mod inspection;
pub(crate) mod lifecycle_watch;
mod replies;
mod requests;
mod view_queries;

#[cfg(test)]
mod render_state_tests;
