---
title: Recover your anymd Pro licence
description: Retrieve an existing anymd Pro licence through Sylphx Money.
money_checkout: pro
---

# Recover your anymd Pro licence

Enter the email address used at checkout. If a paid licence exists, Money sends
an expiring recovery link. The response never says whether that address has a
purchase, and no licence token is shown to an unverified browser.

Open the recovery link within one hour to view and copy your licence token,
then activate it on your machine:

```sh
anymd pro activate '<your token>'
anymd pro status
```

For an MCP server or CI, set `ANYMD_PRO_TOKEN` instead. Existing licences keep
their original product, plan, seats and expiry. Do not share a licence token or
recovery link in a public issue, log or repository.
