# Model Choice Tests

Tests for parsing model responses into harness decisions and validating those
decisions against the primitive tool registry.

## Files

- `contracts_test.rs` verifies the rendered model-choice and loop contracts.
- `parsing_messages_test.rs` covers plain text and mixed prose/control JSON.
- `parsing_answer_now_test.rs` covers fallback `answer_now` parsing.
- `parsing_single_request_test.rs` covers one structured primitive request.
- `parsing_batch_test.rs` covers structured request batches.
- `parsing_wrappers_test.rs` covers fences and provider tool-call markers.
- `parsing_validation_test.rs` covers registry and disabled-tool validation.
