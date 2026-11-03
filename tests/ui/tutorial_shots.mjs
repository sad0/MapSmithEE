// Screenshots for docs/TUTORIAL.md, taken from the running dev app (see cdp.mjs). Only looks: nothing is moved or
// painted, and the light is put back to the map's own hour.
//   node tests/ui/tutorial_shots.mjs [out dir, default docs/img]
import { chromium } from 'playwright-core';

const out = process.argv[2] || 'docs/img';
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.CDP_PORT || 9335}`);
const page = browser.contexts()[0].pages().find((p) => !p.url().startsWith('devtools'));
const js = (f) => page.evaluate(f);
const wait = (ms) => page.waitForTimeout(ms);
const shot = async (name) => { await wait(900); await page.screenshot({ path: `${out}/${name}.jpg`, quality: 88 }); console.log(name); };
const click = (text) => page.getByText(text, { exact: true }).first().click();

const hour = await js('S.time');
await js("document.getElementById('rdBack').style.display = 'none'; select(null); setView(false); setTool('sel'); placeBG(); camBG.zoom = 1.75; camBG.updateProjectionMatrix()");
await shot('bg-view');

// the whole camp from above, south side, like a player would first look at it
await js(`setView(true); camFree.position.set(W / 2, 48, D + 30); orbit.target.set(W / 2, 0, D / 2 - 2); orbit.update()`);
await shot('free-view');
await js('setView(false)');

// one tent selected: gizmo and its own settings (canvas colours, size)
await js(`(() => { const c = new THREE.Vector3(W / 2, 0, D / 2);
  const t = objs.filter(o => tentLook(o)).sort((a, b) => a.node.position.distanceTo(c) - b.node.position.distanceTo(c))[0]; select(t); })()`);
await shot('select-tent');

// a group: every piece around the camp fire
await js(`(() => { const f = objs.find(o => o.it.m === 'flame') || objs[0];
  selectMany(objs.filter(o => o.it.m !== 'flame' && !o.it.brush && o.node.position.distanceTo(f.node.position) < 4.5)); })()`);
await shot('group');
await js('select(null)');

await click('Paint ground');
await js(`layer = GROUNDS.findIndex(g => g.id === 'dirt'); showAdj()`);
await shot('paint-ground');

await click('Tree brush');
await shot('tree-brush');

await click('Camp spots');
await shot('camp-spots');

await click('Walkable');
await shot('walkable');

await click('Select (Q)');
await js('setTime(22)');
await shot('night');
await js(`setTime(${hour})`);

await click('Render area…');
await wait(1500);
await js("document.getElementById('rdTarget').value = 'generic'; document.getElementById('rdName').value = 'AR9900'; document.getElementById('rdTarget').onchange()");
await shot('render-dialog');
await js("document.getElementById('rdBack').style.display = 'none'");
await js('camBG.zoom = 1; camBG.updateProjectionMatrix()');
await browser.close();
