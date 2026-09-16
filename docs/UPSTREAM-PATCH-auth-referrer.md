# Retired login patch: login page `Referrer-Policy` fix (now upstream)

## Why

Upstream `shared/authentication/web.py` serves the password login page with
`Referrer-Policy: no-referrer`. Chrome answers that by sending a literal
`Origin: null` on the login form POST — and upstream's `same_origin()` gate
rejects any present-but-mismatched Origin with
`{"detail":"Cross-origin request rejected."}` (403), before the password is
ever checked. Result: password login is deterministically impossible in
Chrome, with any password, on any fresh tab. Same failure class as
rails/rails#30658, rails/rails#28299, MichaIng/DietPi#8223.

## Change (was `C:/Wan2GP/shared/authentication/web.py` — REVERTED, see below)

- Had added `LOGIN_PAGE_HEADERS = dict(PAGE_HEADERS, {"Referrer-Policy": "same-origin"})`
- Had `login_page()` use it instead of `PAGE_HEADERS`.
- `same-origin` still suppresses cross-origin Referers; only same-origin
  navigations (the login POST to self — no secrets in its URL) carry origin
  info again. Both server-side checks stay intact.

## Verify (after Deepy Web Stop + Start — restart required)

- `curl -sI http://localhost:<deepyPort>/auth/login | grep -i referrer`
  → `Referrer-Policy: same-origin`
- Chrome: fresh tab → Same-PC URL → sign in → chat loads (was 403 before).
- Firefox unchanged (never sent Origin on navigational POSTs).

## Maintenance

- FIXED UPSTREAM 2026-09-16 in deepbeepmeep/Wan2GP@38d4a64
  (`headers = {**PAGE_HEADERS, "Referrer-Policy": "same-origin"}` in
  `login_page()` — identical effect to this retired patch). Pulling that
  Wan2GP update delivers pristine upstream code with working Chrome login.
- RETIRED 2026-09-16 per standing rule (never modify Wan2GP originals):
  the local `web.py` edit was reverted to pristine upstream and the
  launcher-side companion plugin (`wan2gp-login-fix`) was removed entirely.
  C:/Wan2GP shows zero tracked modifications; `git pull` is a pure
  fast-forward again.
- This file is kept as history only and can be deleted.
