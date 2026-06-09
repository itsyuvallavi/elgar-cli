# Provider Visible

This folder decides whether raw provider text should become visible assistant
text.

It does not call the model and it does not render the TUI. It only cleans text
returned by a provider:

- trims empty output
- removes chat-template channel markers
- drops raw tool protocol text

This protects visible assistant output from showing internal protocol fragments
as a normal answer.
