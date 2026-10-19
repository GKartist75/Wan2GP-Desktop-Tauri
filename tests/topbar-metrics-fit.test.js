/**
 * topbar-metrics-fit.test.js — VRAM is never the metric that gets clipped.
 *
 * The metrics row is a flex item inside the topbar and it used to answer a tight
 * bar by shrinking and hiding its own overflow: the *last* metric, VRAM, was cut
 * mid-label ("VRA") exactly when a generation was using it. The fix is a ladder
 * of breakpoints that drops whole readings — sparklines first, then RAM, then
 * CPU/GPU — and protects the VRAM sparkline longest. The embedded Wan2GP view
 * (Back to Dashboard / Console / zoom slider) costs ~330px more, so it runs the
 * same ladder one rung higher behind `.wv-active`.
 *
 * Layout itself is measured in a browser; this pins the contract that made the
 * measurement possible: the ids/classes the ladder targets and the order of the
 * rungs. If a rule is renamed or a metric loses its id, this fails instead of
 * silently going back to clipping.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const css = fs.readFileSync(path.join(SRC, 'style.css'), 'utf8')
const launch = fs.readFileSync(path.join(SRC, 'launchinfo-tab.js'), 'utf8')

function rule(media, selector) {
  // `[^}]*` is enough for these one-declaration rules.
  const m = css.match(
    new RegExp(`@media \\(max-width: ${media}px\\) \\{\\r?\\n[^{}]*${selector}[^{}]*\\{[^}]*\\}`),
  )
  return m ? m[0] : null
}

test('every metric the ladder addresses has an id, and VRAM carries the protected class', () => {
  for (const id of ['metricCpu', 'metricGpu', 'metricGpu2', 'metricRam', 'metricVram', 'metricVram2']) {
    assert.match(html, new RegExp(`id="${id}"`), `${id} missing from index.html`)
  }
  assert.match(
    html,
    /class="metric metric-vram" id="metricVram"/,
    'VRAM must carry metric-vram — that class is what the sparkline ladder spares',
  )
})

test('the sparkline ladder spares VRAM and drops readings in priority order', () => {
  // Dashboard: sparklines except VRAM go first, then RAM/GPU2, then CPU/GPU,
  // then the VRAM graph itself, then the whole cluster.
  assert.match(css, /@media \(max-width: 1450px\) \{\r?\n \.metric:not\(\.metric-vram\) \.metric-spark \{/)
  assert.ok(rule(1380, '#metricRam'), 'RAM rung missing')
  assert.ok(rule(1270, '#metricCpu'), 'CPU/GPU rung missing')
  assert.ok(rule(1150, '.metric-vram .metric-spark'), 'VRAM sparkline rung missing')
  assert.ok(rule(1060, '#topbarMetrics'), 'final dashboard rung missing')

  // Embedded view: same ladder, higher thresholds, and VRAM survives longest.
  assert.match(css, /#dashboard\.wv-active \.metric:not\(\.metric-vram\) \.metric-spark \{/)
  assert.ok(rule(1760, '#dashboard.wv-active #metricCpu'), 'embedded CPU/RAM rung missing')
  assert.ok(rule(1540, '#dashboard.wv-active #metricGpu'), 'embedded GPU rung missing')
  assert.ok(rule(1470, '#dashboard.wv-active .metric-vram .metric-spark'), 'embedded VRAM sparkline rung missing')
  assert.ok(rule(1440, '#dashboard.wv-active #topbarMetrics'), 'embedded final rung missing')

  // The embedded thresholds must be the wider ones: it is the same bar with
  // ~330px less room, so a rung may never fire earlier there than on the
  // dashboard (that would hide data while there is still room for it).
  const pairs = [
    [1760, 1450],
    [1540, 1380],
    [1470, 1270],
    [1440, 1150],
  ]
  for (const [wv, dash] of pairs) {
    assert.ok(wv > dash, `embedded rung ${wv} must start before dashboard rung ${dash}`)
  }
})

test('the embedded view marks the dashboard so the ladder can key off it', () => {
  const show = launch.match(/function showWebviewUI\(\) \{[\s\S]*?\n\}/)
  const hide = launch.match(/function hideWebviewUI\(\) \{[\s\S]*?\n\}/)
  assert.ok(show && hide, 'showWebviewUI/hideWebviewUI must exist')
  assert.match(show[0], /classList\.add\("wv-active"\)/)
  assert.match(hide[0], /classList\.remove\("wv-active"\)/)
})
