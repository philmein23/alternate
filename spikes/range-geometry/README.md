# Range geometry spike

Checks whether an app can read on-screen boxes for byte ranges of an editable `text_input`.

    cargo test --release -- --nocapture --test-threads=1
    cargo run --release -- --snap out.png --scene wrapped

Single-line targets work on public APIs. Wrapped targets report one union box, and keyed reads get expensive quickly (31 ms per edit at 40 keys), so the app needs a small Rune addition.
