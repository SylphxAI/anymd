# anymd Pro

anymd stays MIT and free forever. Everything you can do today (reading, outlining, searching and inspecting documents, OCR, transcripts, the MCP server and the CLI) stays free and ungated.

anymd Pro is an optional licence that unlocks new operations only:

- **Video evidence**: `inspect` with `video_timeline` and `render_frame`, to cite what a video shows.
- **Cite-check**: `inspect` with `cite_check`, to verify that a quotation or citation matches its source.

## Price

US$29, once.

[Buy anymd Pro](https://PRO_PAYMENT_LINK)

You receive a licence token by email.

## Activate

Either set the token in the environment:

```bash
export ANYMD_PRO_TOKEN='<your token>'
```

or store it once:

```bash
anymd pro activate '<your token>'
anymd pro status
```

`activate` verifies the token and saves it to `<config dir>/anymd/pro-token` (owner-only permissions). The environment variable wins over the file. `status` shows whether Pro is active and its plan and issue date; it never prints the token.

Licences are verified offline against a public key built into anymd. Nothing is sent anywhere, and there is no account to sign in to.

If a Pro operation runs without a licence, anymd says so and links to this page; nothing else changes.
