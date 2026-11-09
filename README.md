# Alternate

A native Mac writing app, built with Rune, for trying different versions of a word, sentence or paragraph in place.

Write as usual. Click the word count to turn on the marks and tools. Press ⌘A on a word to add alternatives, then rest the pointer on it and use the arrow keys to swap them on the page. Everything is saved in one Markdown file, with the alternatives in a hidden comment at the end.

## Install

```
scripts/install
```

This builds a release, signs it, and puts it in `~/Applications`. It needs the patched Rune checkout named in `Cargo.toml`.

## AI

Open **AI** at the bottom of the window and choose how to reach Claude:

- **Claude Code sign-in** uses the login `claude` already has on this Mac.
- **Subscription token** uses a token from `claude setup-token`. It stays in your Keychain and is only ever given to the local `claude` command.
- **API key** calls the Anthropic API directly and is billed to that key.

To check a route from Terminal: `alternate --ai-check "Say hello."`

## Develop

`cargo test` runs the suite. `cargo run -- --snap out.png --scene marks` renders a scene without opening a window.
