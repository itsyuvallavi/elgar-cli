# Harness, Tool Loop, And Bottleneck Analysis

Date: 2026-06-02
Related issue: ELG-361
Last updated: 2026-06-02 23:08 WEST

## Executive Summary

Elgar's runtime has a strong truth model but a weak tool-result feedback loop.

The harness verifies shell/file truth correctly. The TUI can render verified
stdout, file reads, tree listings, and raw details. The trace/session log also
stores the exact command, cwd, exit code, stdout tail, stderr tail, and timing.

The bottleneck is that the model does not consistently receive a compact,
actionable version of those verified facts after each tool call. For many shell
commands, the model feedback is effectively:

```text
Executed approved shell command and recorded the verified result.
```

That is true, but it is not enough for the model to answer "the build passed"
or stop calling more tools. The UI sees the rich result; the model often sees a
generic success sentence. This creates repeated tool loops, wasted provider
rounds, and wrong final answers such as asking the user to paste output that
the harness already verified.

The first routing fix helped speed but did not solve reliability:

```text
Before: 81.0s, 11 provider requests, 8 actions, 9 tool calls, 69.4s provider time, 23.7k tokens.
After:  57.9s,  8 provider requests, 6 actions, 7 tool calls, 50.1s provider time, 17.7k tokens.
```

The first provider request is now `tool_enabled`, so the initial plain route
classifier is gone for obvious shell execution. The remaining issue is inside
the tool loop: the model keeps calling tools because it does not receive a
strong enough verified-result contract.

## Source Map

Core runtime:

- `crates/elgar-core/src/agent_loop.rs`
  - Main model/tool loop.
  - Plain route classifier.
  - Deterministic fast paths.
  - Tool definitions selected by intent.
  - Tool-call validation, policy, execution, feedback, and loop continuation.

- `crates/elgar-core/src/model_runtime.rs`
  - Tool schemas and tool-call validation.
  - Defines `shell_command`, file actions, batch creation, guidance.

- `crates/elgar-core/src/controller_reporting.rs`
  - Builds action success messages.
  - Currently gives shell actions generic success text in many cases.

- `crates/elgar-core/src/session.rs`
  - Stores events, actions, verified results, traces, and perf summaries.
  - `TurnPerfSummary` records provider request counts, tool counts, action
    counts, serialized request sizes, tokens, timing, visible text, and thinking.

- `crates/elgar-core/src/provider/lm_studio.rs`
  - LM Studio/OpenAI-compatible provider calls.
  - Tool-enabled calls force `stream = false`.
  - Plain chat can stream when configured.

TUI rendering:

- `crates/elgar-tui/src/shell_result.rs`
  - Renders verified shell results for the user.
  - Shows project trees, file-read code panels, compact shell status, and raw
    details hints.

- `crates/elgar-tui/src/shell_listing.rs`
  - Detects and renders list/tree-style shell output.

Performance tooling:

- `crates/elgar-cli/src/perf.rs`
  - Renders latest trace performance summaries.

- `bin/perf-trace`
  - CLI wrapper for the latest live trace summary.

## Architecture Contract

Elgar's intended contract is:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

This contract is the right foundation. The current bug is not that the harness
verifies too much. The bug is that verified state is not converted into the
right model-facing state at the right time.

The model should decide what it wants to do and write the natural final answer.
The harness should decide whether a proposed action is valid, whether it can
run, what the verified outcome was, and whether the current tool phase should
continue. Those are runtime control decisions, not prose-generation decisions.

The key distinction:

```text
Allowed harness behavior:
- expose or withhold tools by route and phase
- validate tool calls
- execute approved shell/filesystem actions
- store raw details
- render structured verified facts
- send compact verified facts back to the model
- stop a tool phase when the verified goal is complete

Disallowed harness behavior:
- hardcode final assistant prose
- pretend to be the model for normal chat
- cap output tokens just to make examples faster
- infer ordinary natural-language commands through broad phrase tables
```

The proposed performance path stays inside the allowed side. It makes the loop
more deterministic without making the harness write the answer.

## Interaction Layers

Elgar has six relevant layers in the live TUI flow.

### Layer 1: TUI Input And Local Slash Commands

The TUI owns input capture and local commands such as:

```text
/approve
/reject
/copy raw
/details last
/permissions
/exit
```

These are local controls. They should not go to the model as ordinary prompts.

Normal user text, including `hello`, `review this project`, and
`Run npm run build and report the result`, enters the runtime as a turn.

### Layer 2: Agent Runtime Routing

The runtime decides which provider path to use:

```text
plain_chat
tool_enabled
project_review_synthesis
tool_result_synthesis
state/local
```

This is not the same as deciding the final answer. Routing controls which tools
the model can see and what context it receives.

Plain chat should be cheap and tool-free:

```text
user -> provider with no tools -> model-authored answer
```

Tool execution should be phase-based:

```text
user -> provider with narrow tools -> model drafts action
runtime executes/verify -> provider with verified digest and no tools
```

The current implementation still lets many tool turns remain in a repeated
tool-enabled phase after a conclusive shell result exists.

### Layer 3: Provider Request Construction

For a tool-enabled request, Elgar sends:

```text
system prompt
runtime/project context
recent conversation context
verified memory context
optional mode instruction
user input
tool schemas
```

Each tool round appends more messages and sends the whole accumulated request
again. On local 35B models, this matters. A second provider round is not just a
small control decision. It can cost several seconds and thousands of tokens.

The shell fast path helped because it removed one provider request before tool
execution. The next improvement must remove unnecessary tool rounds after the
first verified result.

### Layer 4: Model Drafts Tool Calls

The model does not run tools. It proposes typed JSON calls. For example:

```text
shell_command({ command: "npm run build", timeout_seconds: 120 })
```

That proposed action is untrusted until runtime validation passes.

This distinction matters because we can optimize after this point using
validated command metadata. Classifying the actual command `npm run build` as a
build command is not the same as hardcoding natural-language trigger phrases.
The model already selected the command. The runtime is classifying a structured
action for loop control.

### Layer 5: Policy And Executors

The policy layer decides whether the validated action can run:

```text
review_all -> wait for /approve
auto_create_review_modify -> apply some actions, review others
full_access -> apply validated actions
```

The executor then produces verified facts:

```text
command
cwd
exit code
elapsed time
timed out flag
stdout
stderr
truncation
created/modified path facts
```

This is where truth enters the system. After this point, the provider's earlier
claims are less important than the verified result.

### Layer 6: Reporting To UI, Trace, And Model

Today, the same verified result flows to three consumers:

```text
UI: rich compact rendering
trace/session: raw durable truth
model feedback: often generic success text
```

This asymmetry is the root of the current issue.

The UI can show:

```text
Read file · app/page.tsx · 12 lines · exit 0 · 25ms
```

The trace can store exact stdout. But the model may only get:

```text
Executed approved shell command and recorded the verified result.
```

That model feedback is too lossy. The model cannot reliably summarize, stop,
or avoid redundant tools if it does not receive the facts needed to do so.

## Current Data Flow Diagram

```text
User input
  |
  v
TUI command/input layer
  |
  v
AgentRuntime::turn
  |
  +--> slash command/local state path
  |
  +--> deterministic fast path
  |
  +--> plain route classifier
          |
          v
      run_agent_tool_chat
          |
          v
      provider request with tools
          |
          v
      model tool call draft
          |
          v
      runtime validation
          |
          v
      policy decision
          |
          v
      executor verified result
          |
          +--> session trace/raw truth
          |
          +--> TUI compact rendering
          |
          +--> model feedback message
                    |
                    v
              next provider tool round
```

The speed bug is at the bottom of this diagram. After a verified result exists,
the loop usually goes back to "next provider tool round" instead of switching
to a final no-tool synthesis phase.

## Desired Data Flow Diagram

```text
User asks to run/report command
  |
  v
Narrow shell-execution tool phase
  |
  v
Model drafts one shell command
  |
  v
Runtime validates and executor verifies
  |
  v
VerifiedShellDigest
  |
  +--> TUI compact result
  |
  +--> trace/raw details
  |
  +--> no-tool model synthesis request
            |
            v
      model-authored final answer
```

This preserves model-authored prose but removes redundant tool opportunities
after the command result is already conclusive.

## Current Runtime Flow

### 1. User Input Enters AgentRuntime

The live CLI/TUI path calls:

```text
AgentRuntime::turn(...)
  -> run_agent_turn_with_policy(...)
```

The turn records:

- user message
- reasoning trace start
- current event/action start indexes

Then it chooses one of these paths:

```text
explicit /tool command
deterministic fast path
plain model route classifier
```

### 2. Explicit `/tool` Path

`/tool <request>` skips the plain route classifier and enters the tool loop with
`explicit_tool_command = true`.

This path is more direct and usually faster for command execution. It also has
extra guardrails for repeated read-only inspection.

Observed behavior:

```text
/tool run npm run build
-> proposes shell command
-> /approve runs command
-> build exit 0 in about 4.6s
```

Limitation:

The approval step is local and does not currently produce a model-authored
final pass/fail summary. The user sees verified TUI result rows, not a normal
agent summary.

### 3. Deterministic Fast Path

Current deterministic fast paths are narrow and intentionally conservative:

- exact project-review phrases, such as `review this project`
- obvious shell execution requests, such as `Run npm run build and report...`

The shell fast path was added as a first step for ELG-361. It avoids the plain
classifier for command-shaped execution and exposes only:

```text
ask_guidance
shell_command
```

This reduced the live natural build/report case from 11 provider requests to 8.

Important boundary:

Questions like this must remain plain chat:

```text
What does cargo test do?
```

The current test coverage protects that.

### 4. Plain Route Classifier

If no fast path applies, Elgar asks the model to classify the request with a
small JSON-only prompt.

The classifier can return:

```json
{"route":"chat","content":"..."}
{"route":"execute","intent":"shell_execution"}
{"route":"execute","intent":"project_review"}
{"route":"execute","intent":"plan_execution"}
{"route":"state","answer_kind":"..."}
{"route":"ask_guidance","question":"..."}
```

This is cheap for smaller models, but not always cheap for Qwen 35B in LM
Studio. It can also fail badly when the model thinks too long.

Observed failed case:

```text
Prompt:
Review this Next.js project for production readiness. Inspect package.json,
app/page.tsx, app/layout.tsx, tailwind.config.ts, next.config.mjs, and
postcss.config.mjs. Give 3 concrete findings max. Do not edit files.

Result:
120s timeout, stayed in plain_chat, 0 tools.
```

### 5. Tool Loop

Once execution is selected, `run_agent_tool_chat` builds a message list:

```text
system: AGENT_SYSTEM_PROMPT
system: runtime location/context
system: recent conversation context
system: verified memory context
system: optional explicit tool / project review instructions
user: original input
```

Then for each tool round:

```text
provider tool request
parse assistant text + tool calls
validate tool names and arguments
resolve paths / anchor project roots
apply plan and policy guards
apply action or ask guidance
append tool feedback message
continue or stop
```

The loop can run up to:

```text
MAX_AGENT_TOOL_ROUNDS = 16
```

The natural build/report case used 8 provider requests after the routing fix,
so it stayed well below the max but still wasted substantial time.

## How Tools And Truth Interact

### Model Tool Call Is A Draft

The model does not execute anything. It drafts typed tool calls:

```text
shell_command
create_file
overwrite_file
patch_file
delete_file
move_file
ask_guidance
```

The harness validates these calls before any action can happen.

### Runtime Validates And Resolves

The runtime checks:

- known tool name
- required JSON arguments
- path safety
- allowed roots
- shell command safety
- policy mode
- verified plan contract, when relevant

At this stage, provider prose is not truth. It is only a proposal.

### Policy Decides

Depending on permission mode:

- `review_all` proposes actions and waits for approval.
- `auto_create_review_modify` auto-applies safe create/read-only shell paths,
  but reviews more sensitive actions.
- `full_access` auto-applies validated file and shell actions.

### Executors Verify

After applying an action, the executor records verified facts.

For shell commands, the verified result includes:

- command
- cwd
- elapsed millis
- exit code
- timed out flag
- stdout
- stderr
- truncation flags
- optional verified effect

This is the real source of truth.

### UI Reports

The TUI renders verified truth with specialized logic:

- `show me the project tree` -> project tree/list display
- `cat app/page.tsx` -> code panel
- generic command -> compact "Tool result" row
- raw details remain available through `/details last` and `/copy raw`

The UI is already ahead of the model feedback path.

## Critical Split: UI Truth Vs Model Feedback

This is the main design gap.

The UI receives `VerifiedActionResult::Shell` and can render rich output. The
trace also records stdout/stderr tails.

The model loop often receives only the result string returned by
`apply_agent_action_with_policy`, which is built from
`verified_action_success_message`.

For shell commands without expected paths, the message is:

```text
Executed approved shell command and recorded the verified result.
```

For shell commands with expected paths, the message is:

```text
Executed approved shell command and verified expected paths.
```

These strings are accurate but underspecified. They do not tell the model:

- exit code
- command duration
- stdout summary
- stderr summary
- build/lint/test pass/fail
- whether there is enough information to answer now
- whether another shell command is redundant

This explains the live failure:

```text
The build passed with exit 0 and stdout showed successful compilation.
The model still asked the user to paste output.
```

The model was not necessarily "ignoring truth" maliciously; the loop did not
give it the same truth summary the UI had.

## Live Evidence

### Plain Chat Baseline

Prompt:

```text
hello!
```

Observed:

```text
route: chat
provider_requests: 1
tools_exposed: 0
provider_time_ms: 12339
tokens: 704
```

Meaning:

Plain chat is still model-limited. The harness does little here. Qwen's
reasoning/completion length dominates latency.

### Project Review Fast Path

Prompt:

```text
review this project
```

Observed:

```text
route: execute
provider_requests: 1
actions: 4
provider_time_ms: 26341
tokens: 1806
```

Meaning:

The harness did deterministic file inspection first, then asked for a synthesis.
This is reliable enough behaviorally, but still slow because Qwen synthesis is
slow.

### Advanced Natural Review

Prompt:

```text
Review this Next.js project for production readiness. Inspect package.json,
app/page.tsx, app/layout.tsx, tailwind.config.ts, next.config.mjs, and
postcss.config.mjs. Give 3 concrete findings max. Do not edit files.
```

Observed:

```text
provider_requests: 1
request_mode: plain_chat
tools_exposed: 0
result: 120s timeout
```

Meaning:

The plain route classifier is too risky for some long natural prompts on this
model. It can spend the entire timeout before tools are even available.

### Natural Build/Report Before Shell Fast Path

Prompt:

```text
/permissions full_access
Run npm run build and report the result. Do not edit files.
```

Observed:

```text
visible turn: 81.0s
provider_requests: 11
actions: 8
tool_calls: 9
provider_time_ms: 69362
tokens: 23700
```

The build itself took around 4 seconds. The rest was provider/tool-loop churn.

### Natural Build/Report After Shell Fast Path

Same prompt.

Observed:

```text
visible turn: 57.9s
provider_requests: 8
actions: 6
tool_calls: 7
provider_time_ms: 50137
tokens: 17717
```

Improvement:

- Initial plain classifier removed.
- Provider rounds reduced by 3.
- Provider time reduced by about 19 seconds.
- Total token use reduced by about 5.9k.

Still broken:

- The model ran extra checks after the build.
- It still asked for pasted output despite verified shell output.

## Current Bottlenecks

### Bottleneck 1: Provider Round Count

Each tool round sends the accumulated message history back to LM Studio. As the
loop grows, each request becomes larger:

```text
request 1: messages 4,  bytes 6614
request 8: messages 18, bytes 9887
```

For Qwen 35B, each additional provider request costs seconds even when the tool
itself runs in milliseconds.

The natural build/report after the first fix still used:

```text
8 provider requests
6 actions
7 tool calls
50.1s provider time
```

### Bottleneck 2: Repeated Shell Calls

The model tends to verify the same concept repeatedly:

```text
npm run build
ls .next
npm run build again
test -d .next
cat package.json
pwd && ls .next
ask_guidance
final response
```

Some commands are not byte-for-byte duplicates, so exact duplicate detection is
not enough. They are semantically redundant for the user's goal.

The harness needs a goal-level result, not only command-level duplicate
detection.

### Bottleneck 3: Generic Tool Feedback To Model

After shell execution, the model needs compact verified facts. It currently
often gets a generic message.

Bad model feedback shape:

```text
Executed approved shell command and recorded the verified result.
```

Better feedback shape:

```text
VERIFIED_SHELL_RESULT
command: npm run build
cwd: /Users/yuval/__git/elgar/playground/Nextjs-1
exit_code: 0
elapsed: 3.8s
stdout_summary:
- Next.js compiled successfully
- TypeScript finished
- static pages generated
stderr_summary: empty
answer_now: yes
do_not_call_more_tools_for_this_build_result: yes
```

This would not hardcode visible assistant prose. It would give the model the
truth it needs to write its own answer.

### Bottleneck 4: No Terminal Condition For Goal Satisfaction

The loop currently stops when:

- no tool calls
- max rounds
- pending approval
- explicit repeated/skip guard
- some plan-specific conditions
- certain explicit-tool shell synthesis paths

It does not have a general "the user asked to run build and report; build
completed with exit 0; enough evidence exists; stop tools and synthesize"
condition.

The model is left to decide whether the result is enough. With Qwen, it often
keeps probing.

### Bottleneck 5: Tool-Enabled Calls Are Non-Streaming

LM Studio provider code forces:

```text
config.stream = false
```

for tool-enabled calls.

This means we do not get first-token progress on tool rounds, and the user
waits for the entire provider response before the next tool action. This does
not explain all latency, but it makes the UI feel worse.

### Bottleneck 6: Tool Schema Size And Tool Choice

The first-step shell fast path exposes only:

```text
ask_guidance
shell_command
```

That is better than exposing the full tool set. However, repeated rounds still
send both tool definitions plus accumulated conversation each time.

For specific verified command workflows, even this may be more than needed
after the command has already run. A synthesis-only follow-up with no tools
would be cheaper and safer.

## Why The First Step Helped But Was Not Enough

The first step removed the initial classifier round for obvious shell execution.

Before:

```text
plain_chat classifier
tool_enabled round 1
tool_enabled round 2
...
```

After:

```text
tool_enabled round 1
tool_enabled round 2
...
```

This saves one expensive provider request and prevents classifier timeout for
some prompts.

But the model still controls when to stop tool-calling. Since it receives weak
result feedback, it keeps asking for more tools.

So the next speed improvement is not more routing. It is tool-loop
termination and result grounding.

## Recommended New Method

The right direction is to make the tool loop more state-machine-like.

Do not rely on the model to infer completion from generic prose. The harness
should maintain a verified command goal state and use that to decide when to
switch from tool execution to answer synthesis.

## Alternative Methods Considered

### Option A: Faster Model Or Provider Settings Only

This can help, but it does not fix the harness bug.

LM Studio can answer a trivial prompt faster than Elgar when the request is
small and direct. That shows the provider can be fast under the right context
shape. But the natural build/report turn did not spend most of its time inside
`npm run build`. It spent most of its time in repeated provider requests.

Changing model settings may reduce each request by a few seconds. It will not
prevent 8 to 11 provider requests from happening.

Verdict:

```text
Useful later, not sufficient.
```

### Option B: Output Token Caps

This was rejected.

The user's latency complaint used `hello` as a simple example, but Elgar must
handle unknown request sizes. A hard output cap would make some prompts faster
by making legitimate long answers impossible.

Verdict:

```text
Do not use as the core fix.
```

### Option C: Hardcoded Harness Replies

This was rejected.

Printing a local canned answer such as `The build passed` would be fast, but it
would violate the product contract. The final natural response must come from
the model.

The harness may render a structured verified row:

```text
shell command finished · exit 0 · 4.8s
```

But the normal assistant sentence should be model-authored from verified facts.

Verdict:

```text
Do not do this.
```

### Option D: More Natural-Language Fast Paths

This is risky.

The first shell fast path was intentionally narrow. Expanding this into broad
phrase tables would recreate the old controller problem, where the harness
guessed user intent from English.

Some deterministic routing is acceptable when it is based on command shape or
explicit slash commands. But ordinary prose should remain model-owned.

Verdict:

```text
Use sparingly. Do not make this the main strategy.
```

### Option E: Transactional Tool Turn

This is the preferred method.

Treat a simple shell/report request as a transaction:

```text
1. model chooses command
2. runtime validates command
3. executor runs and verifies command
4. runtime creates compact verified digest
5. runtime closes the tool phase
6. model writes final answer with no tools exposed
```

This preserves model authorship, avoids token caps, avoids broad phrase tables,
and attacks the real bottleneck: repeated provider/tool rounds.

Verdict:

```text
Implement first.
```

### Method: Goal-State Tool Loop

Introduce a small goal state for shell execution turns:

```text
ShellExecutionGoal
- requested_command_family: build | test | lint | install | generic
- primary_command: npm run build
- report_requested: true
- edit_allowed: false
- primary_result: optional verified shell result
- enough_to_answer: bool
```

The model can still choose the command. The harness does not hardcode the final
answer. But after a verified result exists, the harness can decide:

```text
The requested command ran.
The command exited 0 or nonzero.
The user asked to report the result.
No edit was requested.
Therefore stop tool calls and request final synthesis with verified facts.
```

### Method: Verified Result Digest

Convert every verified shell result into a compact digest for model feedback.

Example:

```text
VERIFIED_SHELL_RESULT
action_id: action-1
command: npm run build
cwd: /Users/yuval/__git/elgar/playground/Nextjs-1
exit_code: 0
elapsed_millis: 3816
timed_out: false
stdout:
  first_lines:
    > nextjs-1@0.1.0 build
    > next build
    Next.js 16.2.6 (Turbopack)
  signals:
    compiled successfully
    TypeScript finished
    generated static pages
stderr: empty
result_class: success
answer_now: true
```

For failure:

```text
VERIFIED_SHELL_RESULT
command: npm run build
exit_code: 1
stderr_or_stdout_error_excerpt:
  ...
result_class: failure
answer_now: true
```

This digest should be sent as the tool result content. The raw stdout still
stays in trace/session logs and `/details last`.

### Method: Tool Phase Split

After the primary shell command succeeds or fails, stop exposing tools and make
a no-tool synthesis request:

```text
request_mode: tool_result_synthesis
tools: 0
messages:
  system: answer using verified result only
  system: compact verified result digest
  user: original request
```

This prevents the model from calling `ls .next`, `cat package.json`, etc.

It also reduces payload size because the synthesis request can avoid carrying
the whole tool transcript.

### Method: Semantic Duplicate Control

Exact duplicate shell detection is not enough. Add semantic duplicate classes.

For a build/report goal:

```text
class: primary_build
commands:
- npm run build
- npm run build 2>&1
- npm run build 2>&1; echo EXIT_CODE=$?
- npm run build > /tmp/build-output.txt 2>&1
```

Once one primary build command has a verified result, block later primary build
variants unless the previous result was timed out or inconclusive.

For verification probes:

```text
class: build_artifact_probe
commands:
- ls .next
- test -d .next
- find .next ...
```

If primary build already exited 0, these are redundant for a report-only
request and should be skipped.

### Method: Bounded Tool Budget By Intent

Use smaller loop budgets for narrow intents:

```text
shell_execution report-only:
  max primary commands: 1
  max follow-up probes: 0 or 1
  then synthesize

shell_execution debug/fix:
  max commands before synthesis: 3 to 5
  allow file reads and edits only when user requested fixing

project_review:
  deterministic inspection first
  one synthesis

plain chat:
  one plain provider request
```

This is not token capping. It is tool-loop capping by verified runtime state.
The model can still produce a long answer when the task requires it.

## Transactional Shell Phase Design

The first concrete implementation should not try to solve every agent workflow.
It should target report-only shell execution, because that is where the latest
dogfood failure is clear.

### Inputs In Scope

Examples:

```text
Run npm run build and report the result. Do not edit files.
Run cargo test and tell me if it passed.
Run pnpm lint and summarize failures.
```

In these cases, the user asks for execution and reporting, not autonomous
repair.

### Inputs Out Of Scope

Examples:

```text
Run npm run build, fix any errors, and keep going until it passes.
Start the dev server and keep it running.
Debug why this test is flaky.
Review this project and suggest architecture changes.
```

These need more flexible loops. They can still benefit from better verified
digests, but they should not be forced into one-command terminal behavior.

### Shell Transaction State

Add an internal state object scoped to one turn:

```text
ShellTransaction
- original_user_request
- intent: shell_execution
- edit_allowed: bool
- report_only: bool
- primary_command_seen: bool
- primary_command_class: build | test | lint | install | dev_server | generic
- primary_result: optional verified shell result
- result_conclusive: bool
- blocked_reason: optional string
```

The state should be updated from validated tool calls and verified executor
results, not from provider prose.

### Command Classification Boundary

Classify only after a validated shell command exists.

Acceptable examples:

```text
npm run build -> build
pnpm build -> build
cargo test -> test
npm test -> test
pnpm lint -> lint
npm run dev -> dev_server
```

This classification is command metadata. It is not a natural-language trigger
table because it does not route ordinary text directly.

### Conclusive Result Rules

For report-only command execution:

```text
exit_code == 0 and timed_out == false -> conclusive success
exit_code != 0 and timed_out == false -> conclusive failure
timed_out == true -> conclusive timeout
executor error -> conclusive failure unless retry is explicitly justified
```

If the result is conclusive, stop exposing tools and request synthesis.

### Dev Server Exception

Long-running dev-server commands are different.

Commands classified as `dev_server` should not be treated as a normal
report-only shell transaction. They need a background-process contract:

```text
start server
capture URL/logs
run browser/check command
report status
leave process running only if user asked
```

Do not apply the build/test one-command terminalizer to `npm run dev`.

### Synthesis Request Shape

The synthesis request should be smaller than the full tool transcript.

Recommended shape:

```text
system:
  You are writing the final answer for a completed tool action.
  Use only the verified result below.
  Do not claim files were changed unless verified.
  Do not ask the user to paste output already present in the verified result.

system:
  VERIFIED_SHELL_RESULT
  command: npm run build
  cwd: ...
  exit_code: 0
  elapsed_millis: 4800
  timed_out: false
  stdout_summary:
    - build command completed
    - no stderr
  raw_details_available: true

user:
  Run npm run build and report the result. Do not edit files.
```

Tools exposed:

```text
none
```

This gives the model enough information to write:

```text
The build completed successfully. It ran `npm run build` in the current project
and exited with code 0 in 4.8s. I did not edit files.
```

The exact sentence remains model-authored.

### Failure Synthesis Shape

For failures, the digest should include enough diagnostic excerpt for the model
to explain the failure without another command:

```text
VERIFIED_SHELL_RESULT
command: npm run build
exit_code: 1
timed_out: false
stderr_summary:
  - postcss.config.mjs is treated as an ES module
  - module.exports is not available in ES module scope
stdout_tail:
  - failed to compile
result_class: failure
answer_now: true
```

The synthesis can then answer:

```text
The build failed because `postcss.config.mjs` is an ES module but uses
CommonJS `module.exports`.
```

Again, the harness supplies facts; the model writes the prose.

## Expected Performance Shape

For a report-only build command, the desired request pattern is:

```text
provider request 1:
  mode: tool_enabled
  tools: shell_command + ask_guidance
  model drafts npm run build

local action:
  executor runs npm run build
  session stores raw result
  UI renders compact verified result

provider request 2:
  mode: tool_result_synthesis
  tools: none
  model writes final answer from digest
```

Target:

```text
provider_requests: 2
tool_calls: 1
actions: 1
second shell commands: 0
provider_time_ms: roughly one tool-choice request plus one short synthesis
tokens: under 5k for simple build/report
```

Acceptable fallback:

```text
provider_requests: 3
tool_calls: 1 or 2
actions: 1 or 2
```

Anything near the current result remains a failure for this workflow:

```text
provider_requests: 8
actions: 6
tool_calls: 7
tokens: 17.7k
```

## Proposed Implementation Plan

### Step 1: Add Verified Shell Digest

Add a function in core, not TUI:

```text
verified_shell_result_digest(shell: &ShellActionVerification) -> String
```

It should include:

- command
- cwd
- exit code
- elapsed
- timed out
- stdout summary or excerpt
- stderr summary or excerpt
- truncation flags
- result class
- whether this is enough to answer the original request

Keep raw stdout/stderr out of ordinary prompts unless small. For longer output,
include bounded excerpts and important lines.

### Step 2: Feed Digest To Model After Shell Action

Replace generic shell feedback for model tool messages:

Current:

```text
Executed approved shell command and recorded the verified result.
```

New:

```text
<verified shell digest>
```

Visible UI can stay unchanged.

### Step 3: Stop Tool Loop For Report-Only Shell Execution

When:

- intent is `shell_execution`
- at least one shell command verified
- user did not ask to edit/fix
- command result is conclusive

Then:

```text
request tool_result_synthesis
break loop
```

The synthesis request should expose no tools.

### Step 4: Add Semantic Shell Classes

Classify commands after validation, not from natural language:

```text
npm run build -> build
cargo test -> test
pnpm lint -> lint
```

This is not a natural-language trigger table. It is command execution metadata
after the model has already chosen a shell command.

Use it to block semantically redundant command variants in the same turn.

### Step 5: Add Loop Perf Assertions

Add tests that assert request counts, not just success:

```text
Run npm run build and report the result:
- skips plain_chat classifier
- first request is tool_enabled
- after verified shell result, requests one tool_result_synthesis
- does not issue a second shell_command
```

### Step 6: Live Dogfood

Run these exact prompts:

```text
/permissions full_access
Run npm run build and report the result. Do not edit files.
/exit
```

Expected:

```text
provider_requests: 2 or 3
actions: 1
tool_calls: 1
provider_time_ms: much lower than 50s
visible answer: model-authored pass/fail summary grounded in verified result
```

Also run:

```text
What does cargo test do?
```

Expected:

```text
plain_chat
tools_exposed: 0
```

## What Not To Do

Do not hardcode final assistant replies.

Bad:

```text
if exit_code == 0 { print "The build passed." }
```

Reason:

The user explicitly wants the normal answer from the model. The harness should
provide verified facts, not impersonate the assistant.

Do not cap model output tokens as a latency shortcut.

Reason:

The user may ask for long legitimate output. The issue is loop inefficiency,
not that every answer must be short.

Do not make more natural-language trigger tables.

Reason:

The model owns intent. Deterministic routing should stay limited to command
syntax or explicit slash commands. Command classification after a validated
shell command is acceptable because it is based on the command the model chose,
not on ordinary user prose.

Do not move raw stdout into visible chat by default.

Reason:

The TUI already separates compact display from raw details. Preserve
`/details last` and `/copy raw`.

## Open Questions

### Should shell result synthesis include tools?

Recommendation: no.

After a conclusive report-only shell command, the model should not get tools.
It should answer from verified facts. If it needs more tools, that means the
harness decided too early that the result was conclusive.

### Should the digest include full stdout?

Recommendation: only for small outputs.

For larger outputs, include:

- first few lines
- last few lines
- detected success/error lines
- truncation note
- raw details hint for the UI, not necessarily for the model

### Should "build passed" be visible harness text?

Recommendation: no, not as final assistant prose.

The harness can render structured verified facts in a tool result row:

```text
Tool result
shell command finished · exit 0 · 3.8s
stdout hidden
```

The model should write the natural final answer from the verified digest.

## Current Verdict

Elgar's verification layer is stronger than its model feedback loop.

The current speed problem is not npm, shell execution, or TUI rendering. The
build runs in around 4 seconds. The slow part is repeated provider calls caused
by weak result feedback and missing goal-state termination.

The next improvement should be:

```text
verified shell digest -> stop tools -> no-tool result synthesis
```

That is the highest-leverage path because it addresses speed and correctness at
the same time without hardcoding assistant responses or capping output tokens.
