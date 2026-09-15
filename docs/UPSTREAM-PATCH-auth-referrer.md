# Local Wan2GP patch: login page `Referrer-Policy: same-origin`

## Why

Upstream `shared/authentication/web.py` serves the password login page with
`Referrer-Policy: no-referrer`. Chrome answers that by sending a literal
`Origin: null` on the login form POST — and upstream's `same_origin()` gate
rejects any present-but-mismatched Origin with
`{"detail":"Cross-origin request rejected."}` (403), before the password is
ever checked. Result: password login is deterministically impossible in
Chrome, with any password, on any fresh tab. Same failure class as
rails/rails#30658, rails/rails#28299, MichaIng/DietPi#8223.

## Change (`C:/Wan2GP/shared/authentication/web.py`)

- Added `LOGIN_PAGE_HEADERS = dict(PAGE_HEADERS, {"Referrer-Policy": "same-origin"})`
- `login_page()` uses it instead of `PAGE_HEADERS`.
- `same-origin` still suppresses cross-origin Referers; only same-origin
  navigations (the login POST to self — no secrets in its URL) carry origin
  info again. Both server-side checks stay intact.

## Verify (after Deepy Web Stop + Start — restart required)

- `curl -sI http://localhost:<deepyPort>/auth/login | grep -i referrer`
  → `Referrer-Policy: same-origin`
- Chrome: fresh tab → Same-PC URL → sign in → chat loads (was 403 before).
- Firefox unchanged (never sent Origin on navigational POSTs).

## Maintenance

- Upstream `main` does not contain this fix (checked 2026-09-15); updating
  Wan2GP will show `shared/authentication/web.py` as locally modified and a
  future upstream touch of that file can conflict on `git pull`.
- If a Wan2GP update overwrites it (login 403s in Chrome return), re-apply:
  `git -C C:/Wan2GP diff` to confirm loss, then redo the two-line change
  above (headers dict + `login_page` usage), Stop + Start Deepy Web.
- Proposed upstream fix for deepbeepmeep/Wan2GP: serve the login page with
  `Referrer-Policy: same-origin` (one-line, no gate changes).
