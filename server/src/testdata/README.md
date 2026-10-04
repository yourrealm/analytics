A throwaway RSA key made for tests only (`openssl genpkey -algorithm RSA`).
It grants nothing anywhere. `service-account.json` wraps it the way Google
does; its `token_uri` points elsewhere on purpose, because the server must
ignore it.
