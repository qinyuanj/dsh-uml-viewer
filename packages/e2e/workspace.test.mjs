import assert from 'node:assert/strict'
import test from 'node:test'
import { chooseWorkspace } from './workspace.mjs'

// DSH changes the chooser optimistically, then mounts the created session.
// Keep those two observable UI states separate, as in CI2 run-1pliIh.
function workspacePage(initial = 'shop') {
  let workspace = initial
  let session = `session-${initial}`
  let pending = false
  let observations = 0
  const conversation = {
    first() { return this },
    async getAttribute() {
      if (pending && ++observations === 2) {
        session = `session-${workspace}`
        pending = false
      }
      return session
    },
  }
  const item = {
    last() { return this },
    async click() {
      if (workspace !== 'inspector') {
        workspace = 'inspector'
        pending = true
      }
    },
  }
  const chooser = { async click() {}, async innerText() { return workspace } }
  return {
    getByRole() { return chooser },
    locator(selector) {
      return selector.includes('data-conversation-session') ? conversation : { getByText() { return item } }
    },
    session: () => session,
  }
}

test('workspace choice waits for its conversation before the open prompt can be sent', async () => {
  const page = workspacePage()
  await chooseWorkspace(page, 'inspector')
  assert.equal(page.session(), 'session-inspector')
})

// A second browser page on the same host, as in CI14 run-oAk48s page 2: after
// "New session" the chip shows its "Choose workspace" placeholder until the
// draft session mounts, and DSH mounts it in the last used workspace. Reading
// the conversation waits for that mount, as Playwright's getAttribute does.
function hydratingPage(lastUsed) {
  let session = null
  let workspace = null
  const hydrate = () => {
    if (session === null) {
      workspace = lastUsed
      session = `session-${lastUsed}`
    }
  }
  const conversation = {
    first() { return this },
    async getAttribute() { hydrate(); return session },
  }
  const item = {
    last() { return this },
    async click() {
      hydrate()
      if (workspace !== 'inspector') throw new Error('the fake only offers the last used workspace')
    },
  }
  const chooser = {
    async click() {},
    async innerText() { return session === null ? 'Choose workspace' : workspace },
  }
  return {
    getByRole() { return chooser },
    locator(selector) {
      return selector.includes('data-conversation-session') ? conversation : { getByText() { return item } }
    },
    session: () => session,
  }
}

test('a draft that mounts in the last used workspace is already that workspace', async () => {
  const page = hydratingPage('inspector')
  await chooseWorkspace(page, 'inspector')
  assert.equal(page.session(), 'session-inspector')
})

test('choosing the current workspace keeps its existing conversation', async () => {
  const page = workspacePage('inspector')
  await chooseWorkspace(page, 'inspector')
  assert.equal(page.session(), 'session-inspector')
})
