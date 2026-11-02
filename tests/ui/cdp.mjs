// Drive the running dev app over the WebView2 debug port (npm run tauri dev with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9335). Real mouse events, not synthetic DOM ones.
// node tests/ui/cdp.mjs shot out.png | eval "<js>" | click x y | rclick x y | dblclick x y | drag x1 y1 x2 y2 [shift]
//   | key Control+z | type "text" | wheel x y dy | clicktext "Button text"
import { chromium } from 'playwright-core';

const [cmd, ...a] = process.argv.slice(2);
const port = process.env.CDP_PORT || 9335;
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages().find((p) => !p.url().startsWith('devtools')) ?? browser.contexts()[0].pages()[0];
const n = (i) => Number(a[i]);
try {
  switch (cmd) {
    case 'shot': await page.screenshot({ path: a[0] }); console.log(page.url()); break;
    case 'eval': console.log(JSON.stringify(await page.evaluate(a[0]), null, 1)); break;
    case 'click': await page.mouse.click(n(0), n(1)); break;
    case 'rclick': await page.mouse.click(n(0), n(1), { button: 'right' }); break;
    case 'dblclick': await page.mouse.dblclick(n(0), n(1)); break;
    case 'drag': {
      if (a[4] === 'shift') await page.keyboard.down('Shift');
      await page.mouse.move(n(0), n(1)); await page.mouse.down();
      for (let k = 1; k <= 10; k++) await page.mouse.move(n(0) + ((n(2) - n(0)) * k) / 10, n(1) + ((n(3) - n(1)) * k) / 10);
      await page.mouse.up();
      if (a[4] === 'shift') await page.keyboard.up('Shift');
      break;
    }
    case 'key': await page.keyboard.press(a[0]); break;
    case 'type': await page.keyboard.type(a[0]); break;
    case 'wheel': await page.mouse.move(n(0), n(1)); await page.mouse.wheel(0, n(2)); break;
    case 'clicktext': await page.getByText(a[0], { exact: true }).first().click(); break;
    default: console.log('unknown command');
  }
} finally {
  await browser.close().catch(() => {});
}
