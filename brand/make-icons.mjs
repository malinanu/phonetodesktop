// Renders every Lanyard icon from geometry.mjs. Needs Node 22+, the `playwright` package and a Chromium
// (CHROMIUM_PATH; PLAYWRIGHT_PATH if it is not installed locally), plus ImageMagick `convert` for the Windows .ico. Run from the repo root:
//   CHROMIUM_PATH=/path/to/chromium node brand/make-icons.mjs
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { svg, androidForeground, COLORS } from './geometry.mjs';

// PLAYWRIGHT_PATH may point at a global install (npm root -g)/playwright.
const { chromium } = createRequire(import.meta.url)(process.env.PLAYWRIGHT_PATH || 'playwright');
const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const out = (p) => path.join(root, p);
const write = (p, data) => { fs.mkdirSync(path.dirname(out(p)), { recursive: true }); fs.writeFileSync(out(p), data); };

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, args: ['--no-sandbox'] });
async function png(markup, size, transparent = true) {
  const page = await browser.newPage({ viewport: { width: size, height: size }, deviceScaleFactor: 1 });
  await page.setContent(`<body style="margin:0;background:transparent"><div style="width:${size}px;height:${size}px">${markup.replace(/width="\d+" height="\d+"/, `width="${size}" height="${size}"`)}</div></body>`);
  const buf = await page.screenshot({ omitBackground: transparent, clip: { x: 0, y: 0, width: size, height: size } });
  await page.close();
  return buf;
}

// ---- master artwork
write('brand/lanyard-mark.svg', svg({ bg: 'none' }));
write('brand/lanyard-icon.svg', svg({ bg: 'rounded' }));
write('installer/assets/phone-remote.svg', svg({ bg: 'rounded' }));
write('site/assets/favicon.svg', svg({ bg: 'rounded', scale: 1.08 }));

// ---- desktop (Linux, macOS, Windows)
const rounded = svg({ bg: 'rounded', scale: 1.04 });
write('installer/assets/phone-remote.png', await png(rounded, 512));
write('installer/assets/phone-remote-1024.png', await png(svg({ bg: 'macos', scale: 1.0 }), 1024));
const icoSizes = [16, 24, 32, 48, 64, 128, 256];
const tmp = fs.mkdtempSync(path.join(process.env.TMPDIR || '/tmp', 'lanyard-ico-'));
const icoFiles = [];
for (const s of icoSizes) { const f = path.join(tmp, `${s}.png`); fs.writeFileSync(f, await png(svg({ bg: 'rounded', scale: s <= 32 ? 1.22 : 1.1 }), s)); icoFiles.push(f); }
execFileSync('convert', [...icoFiles, out('installer/assets/lanyard.ico')]);
// Tray icon: 32x32 RGBA bytes the agent embeds (a dark disc is not needed; the mark alone reads on any tray).
const trayPng = await png(svg({ bg: 'rounded', scale: 1.22 }), 32);
const trayRgba = await (async () => {
  const page = await browser.newPage();
  await page.setContent('<canvas id=c width=32 height=32></canvas>');
  const arr = await page.evaluate(async (b64) => {
    const img = new Image(); img.src = 'data:image/png;base64,' + b64; await img.decode();
    const c = document.getElementById('c'); const g = c.getContext('2d'); g.drawImage(img, 0, 0, 32, 32);
    return Array.from(g.getImageData(0, 0, 32, 32).data);
  }, trayPng.toString('base64'));
  await page.close();
  return Buffer.from(arr);
})();
write('agent/assets/tray-32.rgba', trayRgba);

// ---- Android (native app and Flutter app share the artwork)
for (const base of ['android/app/src/main', 'mobile/android/app/src/main']) {
  write(`${base}/res/drawable/ic_fg.xml`, androidForeground());
  write(`${base}/res/mipmap-anydpi-v26/ic_launcher.xml`, `<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_bg" />
    <foreground android:drawable="@drawable/ic_fg" />
    <monochrome android:drawable="@drawable/ic_fg" />
</adaptive-icon>
`);
}
const colorsNative = out('android/app/src/main/res/values/colors.xml');
fs.writeFileSync(colorsNative, fs.readFileSync(colorsNative, 'utf8').replace(/(<color name="ic_bg">)#[0-9A-Fa-f]{6}(<\/color>)/, `$1${COLORS.bg.toUpperCase()}$2`));
write('mobile/android/app/src/main/res/values/ic_colors.xml', `<?xml version="1.0" encoding="utf-8"?>
<resources>
    <color name="ic_bg">${COLORS.bg.toUpperCase()}</color>
</resources>
`);
for (const [dir, s] of Object.entries({ mdpi: 48, hdpi: 72, xhdpi: 96, xxhdpi: 144, xxxhdpi: 192 })) {
  write(`mobile/android/app/src/main/res/mipmap-${dir}/ic_launcher.png`, await png(svg({ bg: 'rounded', scale: 1.04 }), s));
}

// ---- iOS: full-bleed opaque squares (the system rounds the corners)
const setDir = 'mobile/ios/Runner/Assets.xcassets/AppIcon.appiconset';
const contents = JSON.parse(fs.readFileSync(out(`${setDir}/Contents.json`), 'utf8'));
const cache = new Map();
for (const im of contents.images) {
  const px = Math.round(parseFloat(im.size) * parseInt(im.scale));
  if (!cache.has(px)) cache.set(px, await png(svg({ bg: 'square', scale: 1.0 }), px, false));
  write(`${setDir}/${im.filename}`, cache.get(px));
}

// ---- website
write('site/assets/favicon-32.png', await png(svg({ bg: 'rounded', scale: 1.18 }), 32));
write('site/assets/apple-touch-icon.png', await png(svg({ bg: 'square', scale: 1.0 }), 180, false));
write('site/assets/icon-192.png', await png(rounded, 192));
write('site/assets/icon-512.png', await png(rounded, 512));
await browser.close();
console.log('icons written');
