# ChatGPT Desktop

A native Windows desktop application for ChatGPT built with Tauri 2.

## Features

- Direct access to ChatGPT
- Native Windows application
- System tray with minimize-to-tray
- Launch at system startup option
- Multiple login methods (Google, Apple, Microsoft SSO)
- Single instance enforcement

## Screenshots

*(Coming soon)*

## Prerequisites

- **Rust** (latest stable) — download from [rustup.rs](https://rustup.rs/)
- **Node.js** (v18 or later) — download from [nodejs.org](https://nodejs.org/)
- **WebView2** — pre-installed on Windows 10 and later

## Building from Source

1. Clone the repository:
   ```
   git clone https://github.com/nwn900/ChatGPTDesktopApp.git
   ```
2. Navigate to the Tauri backend:
   ```
   cd src-tauri
   ```
3. Build the application:
   ```
   cargo tauri build
   ```

The compiled binary will be placed in `src-tauri/target/release/`.

## Contributing

Contributions are welcome. Submit a Pull Request.

## License

ISC

## Acknowledgments

- [Tauri](https://tauri.app/)
- [OpenAI](https://openai.com/)
- [ChatGPT](https://chatgpt.com/)
