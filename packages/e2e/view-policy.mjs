// Test-only deterministic companion vocabulary, NOT an EDN parser or policy writer.
// Only the lane's Deep scratch policy and exact panel message forms are supported.
export const VIEW_POLICY = `; Companion acceptance: keep this comment and the deliberate spacing. 草稿
{:hierarchical true
 :title "Deep"
 :prefix "deep"
 :sources [{:lang :typescript :root "src" :prefix "deep"}]
 ; Architecture order differs from display order on purpose.
 :levels [[core] [api]]
 :order [api core alpha beta]
 :omit [  ] ; source-only
 :proposals [
   ; Keep the existing proposal before any new one.
   {:id :p-review :name "Review plan"
    :layers [{:id :surface :label "Surface" :nses [api core]}]
    :omit [ ]} ; proposal-only
   ; New proposals go immediately above this comment.
 ]
 :out "diagram.edn"} ; trailing layout stays
`

const heading = 'UML viewer panel request:\n'
const token = '[0-9a-f]{32}'
const omitPattern = new RegExp(`^${heading}\\{:op :omit :target \\{:id :api :kind :class\\} :context (\\{:context :real\\}|\\{:context :proposal :name "Review plan" :proposal-id :p-review\\}) :request "(${token})"\\}$`)

function splice(text, before, after) {
  const start = text.indexOf(before)
  if (start < 0 || text.indexOf(before, start + before.length) >= 0) throw new Error('fixture anchor must occur exactly once')
  return { text: text.slice(0, start) + after + text.slice(start + before.length), start, before, after }
}

export function editViewPolicy(text, message, context = 'real') {
  const proposal = new RegExp(`^${heading}\\{:op :new-proposal :name "([^"\\\\\\r\\n]+)" :request "(${token})"\\}$`).exec(message)
  if (proposal) {
    const name = proposal[1]
    if (!/^[\p{L}\p{N} _:-]{1,80}$/u.test(name)) throw new Error('invalid fixture proposal name')
    if (/^\d{4}-/.test(name)) {
      if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(name)
        || new Date(name.replace(' ', 'T') + 'Z').toISOString().slice(0, 19).replace('T', ' ') !== name) {
        throw new Error('invalid fixture timestamp')
      }
    }
    let number = 1
    while (text.includes(`:p-e2e-${number}`)) number += 1
    const proposalId = `:p-e2e-${number}`
    const anchor = '   ; New proposals go immediately above this comment.'
    return { ...splice(text, anchor, `   {:id ${proposalId} :name "${name}" :layers []}\n${anchor}`), request: proposal[2], context, proposalId }
  }
  const match = omitPattern.exec(message)
  if (!match) throw new Error('unrecognized fixture request')
  const real = match[1] === '{:context :real}'
  const change = real ? splice(text, '[  ] ; source-only', '[ :api ] ; source-only')
    : splice(text, '[ ]} ; proposal-only', '[:api]} ; proposal-only')
  return { ...change, request: match[2], context: real ? 'real' : 'p-review' }
}
