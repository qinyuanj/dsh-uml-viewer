## UML viewer panel (dsh-uml-viewer)

The user has this session's UML viewer panel open. It is bound to this session and shows the
project in the session working directory; the panel actions arrive as ordinary user messages.

- Quality calls are serialized: send one `uml_crap` / `uml_mutate` / `uml_dry` request at a
  time and wait for its result before starting the next one.
- `uml_ir` runs only after the latest quality call in this session returned `validResult`
  true, or when the request needs no quality refresh (such as `:omit` or a proposal request).
- `uml_display` runs only after the latest `uml_ir` in this session succeeded. If it is
  refused, fix the cause and run `uml_ir` again instead of retrying the display.
- `:refresh-mutate` is differential on the target files; `:refresh-mutate-all` passes
  `--mutate-all` for those same files. Pass every target file in one quality call.
- `:omit` adds the id to the current proposal's `:omit`, or to the policy `:omit` on the real
  diagram, then regenerates the IR; display only after the IR succeeds.
- Proposals are named groupings that are not in the source. Preserve them when you rewrite the
  policy, do not invent them, and answer **P** (the panel's namespace-tree action) by returning
  to the namespace tree. A new proposal you are asked to add is named with a timestamp unless
  the request names it.
- Edit the policy file in place: change only the keys you mean to, and keep its comments,
  ordering and line layout. Never edit the generated IR; change the policy and run `uml_ir`.
- A class with no CRAP or mutation data is shown red; never hide it to make the diagram look
  better.
- The discussion context is the diagram under discussion. Stay on it until a later `:context`
  request arrives.
