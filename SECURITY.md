# Security policy

## Reporting a problem

Please report security issues privately with GitHub's **Report a vulnerability** button on the [Security tab](https://github.com/aemrican123/vibdl/security/advisories/new). Don't open a public issue for them.

Include what you found, how to reproduce it, and which version you tested. I'll reply as soon as I can, and credit you in the fix unless you'd rather stay anonymous.

## Supported versions

Only the latest release gets fixes.

## What counts

- API keys or user IDs ending up anywhere other than the OS keychain: files, logs, error messages, screenshots of the UI.
- A board's response (a post, a tag, a file name) being able to run script in the app, read files outside the library folder, or write outside it.
- The app sending data to any host other than the board you are using.

## Please never post your API key

Not in an issue, a log, or a screenshot. VIBDL removes `api_key` and `user_id` from its log, but check anything before you paste it. If a key does leak, reset it on the board's account page.
