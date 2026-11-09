/** Prepare a root view for geometry checks beside the floating inspector. */
export async function panBesideInspector(panel) {
  const canvas = await panel.canvas.boundingBox()
  const inspector = await panel.panel.getByRole('complementary', { name: '检查器' }).boundingBox()
  if (!canvas || !inspector) throw new Error('canvas and inspector must be visible before panning')
  const exposed = inspector.x - canvas.x
  const distance = inspector.width
  if (exposed <= distance) throw new Error('the panel is too narrow for an unobscured inspector pan')
  // Keep both ends in the exposed canvas. This is a normal user gesture;
  // measure a fresh producer frame afterwards, since the camera has moved.
  const start = canvas.x + (exposed + distance) / 2
  const y = canvas.y + canvas.height / 2
  await panel.page.mouse.move(start, y)
  await panel.page.mouse.down()
  await panel.page.mouse.move(start - distance, y, { steps: 6 })
  await panel.page.mouse.up()
}
