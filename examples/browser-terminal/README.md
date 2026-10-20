# Browser terminal

A static-hostable example of a BashKit shell and Ghostty terminal rendered by IPP. Press **Start session** to launch the terminal Worker, then click the Canvas to type. Ctrl+Shift+Escape releases focus. The shell runs locally with an in-memory filesystem.

```sh
python tools/ipp.py build terminal
python -m http.server 8766 --bind 127.0.0.1 --directory target/terminal-build
```

Open `http://localhost:8766/`. Deployable files are self-contained in the build directory and work at a subdirectory without isolation headers. The [terminal package](../../packages/ipp-terminal/README.md) owns the integration contract and limitations. The cockpit uses the same worker and standard protocol.
