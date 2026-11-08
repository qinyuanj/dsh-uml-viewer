import http from 'node:http'
import fs from 'node:fs/promises'
import path from 'node:path'

/** A Messages-compatible scripted endpoint. No request leaves the test host. */
export async function startMockLlm({ log }) {
  await fs.mkdir(log, { recursive: true })
  const requests = []
  const server = http.createServer((req, res) => {
    const chunks = []
    req.on('data', chunk => chunks.push(chunk))
    req.on('end', async () => {
      const body = Buffer.concat(chunks).toString('utf8')
      const url = req.url ?? '/'
      requests.push({ method: req.method, url, body })
      await fs.writeFile(path.join(log, `${String(requests.length).padStart(3, '0')}.json`), body)
      if (!url.startsWith('/v1/messages')) {
        res.writeHead(404, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ error: { message: 'mock LLM only serves /v1/messages' } }))
        return
      }
      const request = JSON.parse(body)
      const serialized = JSON.stringify(request.messages ?? [])
      const reply = !request.tools?.length
        ? { text: 'Isolated UML session' }
        : !serialized.includes('"name":"uml_ir"')
          ? { tool: { name: 'uml_ir', input: {} } }
          : !serialized.includes('Render the sample diagram')
            ? { text: 'Generated the sample project IR.' }
            : !serialized.includes('"name":"uml_display"')
              ? { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
              : { text: 'The isolated mock model displayed the generated IR. No real model was called.' }
      res.writeHead(200, {
        'content-type': 'text/event-stream',
        'cache-control': 'no-cache',
        connection: 'keep-alive',
      })
      const send = event => res.write(`data: ${JSON.stringify(event)}\n\n`)
      send({ type: 'message_start', message: { id: `msg_mock_${requests.length}`, type: 'message', role: 'assistant', model: 'deepseek-flash', content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 1, output_tokens: 1 } } })
      if (reply.tool) {
        send({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id: `toolu_mock_${requests.length}`, name: reply.tool.name, input: {} } })
        send({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(reply.tool.input) } })
      } else {
        send({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } })
        send({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: reply.text } })
      }
      send({ type: 'content_block_stop', index: 0 })
      send({ type: 'message_delta', delta: { stop_reason: reply.tool ? 'tool_use' : 'end_turn', stop_sequence: null }, usage: { output_tokens: 8, input_tokens: 1 } })
      send({ type: 'message_stop' })
      res.end()
    })
  })
  const port = await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => resolve(server.address().port))
  })
  return {
    url: `http://127.0.0.1:${port}`,
    requests,
    stop: () => new Promise(resolve => server.close(resolve)),
  }
}
