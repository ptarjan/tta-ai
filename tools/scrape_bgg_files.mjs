// Download BGG filepage attachments through real Chrome (Playwright).
//
// Why this exists (see docs/EXTERNAL_AIS.md §5c): BGG's /file/* path is behind a
// Cloudflare bot challenge that urllib/curl can never pass, and the signed
// download_redirect URL is absent from the served HTML — it only appears in the
// hydrated DOM. So: real Chrome (channel:'chrome'), session cookies injected from
// the curl cookie jar, read the href, then fetch() it *inside the page* so the
// request carries the browser's Cloudflare clearance.
//
// Usage:  node tools/scrape_bgg_files.mjs
// Needs:  npm i playwright   (run from /tmp/bggwork, or set NODE_PATH)
// Cookie jar: /tmp/bgg_cookies.txt (Netscape format, from the /login/api/v1 POST).

import { chromium } from 'playwright';
import fs from 'fs';
import path from 'path';

const OUT = '/Users/pt/tta-ai/sources';
const JAR = process.env.BGG_JAR || '/tmp/bgg_cookies.txt';
const UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36';

const TARGETS = [
  { fileid: '154670', page: '123302', slug: 'tta-a-new-story-of-civilization-card-reference', out: 'bgg_154670_card_reference_v109' },
  { fileid: '409053', page: '293343', slug: 'through-the-ages-card-counts', out: 'bgg_409053_player_card_counts' },
];

function loadJar(file) {
  const out = [];
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    if (!line.trim() || line.startsWith('#')) continue;
    const f = line.split('\t');
    if (f.length < 7) continue;
    let [domain, , cpath, secure, expires, name, value] = f;
    out.push({
      name, value: value.trim(),
      domain: domain.startsWith('.') ? domain : '.' + domain,
      path: cpath || '/',
      expires: Number(expires) || -1,
      httpOnly: false,
      secure: secure.toUpperCase() === 'TRUE',
      sameSite: 'Lax',
    });
  }
  return out;
}

const cookies = loadJar(JAR);
console.log('loaded cookies:', cookies.map(c => c.name).join(','));

const b = await chromium.launch({ channel: 'chrome', headless: true });
const ctx = await b.newContext({ acceptDownloads: true, userAgent: UA });
await ctx.addCookies(cookies);
const p = await ctx.newPage();

// sanity: are we actually logged in?
const who = await (await ctx.newPage().then(async pg => {
  await pg.goto('https://boardgamegeek.com/api/accounts/current', { waitUntil: 'domcontentloaded' });
  const t = await pg.evaluate(() => document.body.innerText.slice(0, 300));
  await pg.close();
  return { t };
})).t;
console.log('accounts/current:', who);

for (const t of TARGETS) {
  const url = `https://boardgamegeek.com/filepage/${t.page}/${t.slug}`;
  const r = await p.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 });
  console.log(t.fileid, 'filepage status', r && r.status());
  // wait for hydration to inject the signed link
  let href = null;
  for (let i = 0; i < 20; i++) {
    href = await p.$$eval('a[href*="download_redirect"], a[href*="/file/download"]',
      as => (as[0] ? as[0].getAttribute('href') : null)).catch(() => null);
    if (href) break;
    await p.waitForTimeout(1000);
  }
  console.log(t.fileid, 'href', href);
  if (!href) { console.log(t.fileid, 'NO LINK — dumping title:', await p.title()); continue; }

  // Two dead ends, recorded so nobody retries them:
  //  * in-page fetch() of the signed URL -> "TypeError: Failed to fetch". The
  //    download_redirect 307s to *s3.amazonaws.com/geekdo-files.com/bgg<fileid>?...*,
  //    a different origin with no CORS headers, so XHR can never read the body.
  //  * clicking the anchor -> no 'download' event for a PDF; Chrome renders it in the
  //    built-in PDF viewer instead. .xls does download, but relying on that is fragile.
  // What works for both: navigate to the link and grab the S3 response body off the
  // network event. Playwright keeps the body available for the response object.
  //  * response.body() on the S3 response is ALSO wrong for PDFs: Chrome's built-in
  //    PDF viewer answers with its own 536-byte pdf_embedder wrapper HTML, and for the
  //    .xls the navigation invalidates the body before it can be read. This is exactly
  //    how a previous attempt produced a "131 KB HTML page named .xls".
  // The reliable recipe: sniff the *signed S3 URL* off the request log, then download it
  // with a plain node fetch. S3 is not Cloudflare-protected and the signature is in the
  // query string, so no browser and no cookies are needed for the second hop.
  let s3url = null;
  const onReq = (r) => {
    const u = r.url();
    if (!s3url && u.includes('geekdo-files.com') && u.includes('X-Amz-')) s3url = u;
  };
  p.on('request', onReq);
  try {
    await p.goto('https://boardgamegeek.com' + href, { timeout: 60000 });
  } catch (e) {
    console.log(t.fileid, 'goto ended:', e.message.split('\n')[0]);
  }
  for (let i = 0; i < 15 && !s3url; i++) await p.waitForTimeout(1000);
  p.off('request', onReq);
  if (!s3url) { console.log(t.fileid, 'NO S3 URL SEEN'); continue; }
  console.log(t.fileid, 's3', s3url.split('?')[0]);

  const s3res = await fetch(s3url, { headers: { 'User-Agent': UA } });
  console.log(t.fileid, 's3 fetch', s3res.status, s3res.headers.get('content-type'),
    s3res.headers.get('content-length'));
  const buf = Buffer.from(await s3res.arrayBuffer());
  const head = buf.slice(0, 8);
  let ext = '.bin';
  if (head[0] === 0x25 && head[1] === 0x50) ext = '.pdf';                     // %PDF
  else if (head[0] === 0xd0 && head[1] === 0xcf) ext = '.xls';                // OLE2
  else if (head[0] === 0x50 && head[1] === 0x4b) ext = '.xlsx';               // ZIP/OOXML
  else if (buf.slice(0, 200).toString('latin1').toLowerCase().includes('<html')) ext = '.HTML-NOT-A-FILE';
  const dest = path.join(OUT, t.out + ext);
  fs.writeFileSync(dest, buf);
  console.log(t.fileid, 'WROTE', dest, 'magic', [...head].map(x => x.toString(16).padStart(2, '0')).join(' '));
}
await b.close();
