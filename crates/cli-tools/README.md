# localgpt-cli-tools

The tools the [LocalGPT](https://github.com/localgpt-app/localgpt) command line gives its agent on a
desktop: `bash`, `read_file`, `write_file`, `edit_file` and a browser.

They live outside `localgpt-core` because they need the platform sandbox
(`localgpt-sandbox`) and carry security weight a mobile build should not
have. The `localgpt` CLI is what uses them.

License: Apache-2.0.
