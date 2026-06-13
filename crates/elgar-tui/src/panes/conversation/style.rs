//! Conversation line style markers.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ConversationLineStyle {
    #[default]
    Plain,
    Model,
    VerifiedState,
    User,
    Loading,
    Thinking,
    Metrics,
    Details,
}
