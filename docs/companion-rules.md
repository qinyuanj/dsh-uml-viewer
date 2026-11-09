## UML viewer panel (dsh-uml-viewer)

The user has this session's UML viewer panel open. It is bound to this session and shows the
project in the session working directory; the panel actions arrive as ordinary user messages.

Panel refreshes arrive as visible user messages headed `UML viewer panel request:` followed
by EDN, for example:

```edn
{:op :refresh-mutate :target {:id :core :kind :component} :files ["src/core/a.ts" "src/core/b.ts"]}
```

Handle these requests in message order, finishing each request's quality → IR → display
sequence before starting the next. The target identifies the class or namespace component;
`:files` contains its source files relative to this session's project. A proposal component
(`:id :proposal.<group>`) lists the members of that proposal group, so refreshing it changes
no proposal definition.

- `:refresh-crap`: call `uml_crap` once. Standard CRAP uses the declared whole business
  tree (omit `targets`); if `uml-quality.json` declares an operation with target scope,
  pass `:files` as `targets`. Respect the project's declared scope and exclusions.
- `:refresh-mutate`: call `uml_mutate({targets: files})` once, differential.
- `:refresh-mutate-all`: call `uml_mutate({targets: files, mutateAll: true})` once;
  `mutateAll` passes the binary's `--mutate-all` for exactly those files.
- Quality calls are serialized: send one `uml_crap` / `uml_mutate` / `uml_dry` request at a
  time and wait for its result before starting the next one. Pass every target file in
  one call, so mutation shares one coverage run and baseline. Survivor/uncovered findings
  do not authorize reruns or switching differential mutation to full mutation.
- If a quality result has `validResult: false`, stop that request, report its `reason`
  and exit code, and run neither `uml_ir` nor `uml_display`. Continue later queued requests.
  Use the tool's returned version-1 record, never a snapshot result file or a generic
  exit-code rule. The host validates operation, project root and actual exit code.
- `uml_ir` runs only after the latest quality call in this session returned `validResult`
  true, or when the request needs no quality refresh (`:omit`, `:new-proposal`). Those requests
  carry `:request "<id>"`: pass it as `uml_ir({request: "<id>"})` so the IR runs even after an
  earlier failed quality call. Each id works once, in this session only; never reuse one or pass
  it for any other IR.
- `uml_display` runs only after the latest `uml_ir` in this session succeeded. If it is
  refused, fix the cause and run `uml_ir` again instead of retrying the display.
- `{:op :omit :target {:id … :kind …} :context {…} :request "…"}` hides that element: add the
  id to the `:omit` of the proposal its `:context` names, or to the policy `:omit` when the
  context is `{:context :real}`. Then run `uml_ir` with the request id; call `uml_display` only
  after that IR succeeds.
- `{:op :new-proposal :name "…" :request "…"}` adds one proposal with exactly that name (the
  panel's default is a local timestamp) and an empty layer list to the policy `:proposals`, with a
  fresh `:id`; then `uml_ir` with the request id and, after it succeeds, `uml_display`.
- Proposals are named groupings that are not in the source. Preserve them when you rewrite the
  policy, do not invent them, and answer **P** (the panel's namespace-tree action) by returning
  to the namespace tree. A new proposal you are asked to add is named with a timestamp unless
  the request names it.
- Edit the policy file in place: change only the keys you mean to, and keep its comments,
  ordering and line layout. Never edit the generated IR; change the policy and run `uml_ir`.
- A class with no CRAP or mutation data is shown red; never hide it to make the diagram look
  better.
- `{:op :context :context {:context :real}}` or `{:op :context :context {:context :proposal
  :proposal-id … :name …}}` names the discussion context: the diagram under discussion. The panel
  already shows it; change no file. Stay on it, and make later proposal edits there, until a later
  `:context` request arrives.
