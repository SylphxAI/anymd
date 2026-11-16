---
title: Test Pro licence activation
description: Check test licences without changing your production activation.
---

# Test Pro licence activation

Use these commands when testing licence delivery, not when activating a purchase.

## Check your version

Pro requires anymd 8.4.0 or later. Check the installed version:

```sh
npx -y @sylphx/anymd@latest --version
```

## Licence compatibility

The offline production policy accepts the historical seller key and the current
live licence signer. Key rotation adds trust without invalidating sold licences;
signature, plan, product and expiry checks still apply.

## Test activation

For a test licence grant, use `anymd pro --test status` or
`anymd pro --test activate <token>`. This explicit policy trusts only the test
signer, reports `Pro (TEST)`, reads `ANYMD_TEST_PRO_TOKEN`, and stores activations
in `test-pro-token` beside the production `pro-token` file. The ordinary CLI
and MCP Pro operations use production trust and never consume that test file
or variable. Test mode does not start a purchase or establish a paid production
licence. Keep real tokens out of shell history and logs.
