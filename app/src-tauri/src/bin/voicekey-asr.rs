//! 轻量级识别入口：`voicekey-asr --asr <channel> <wav>`。
//!
//! 不链接 Tauri/GTK/WebKit，供 Fcitx5 模块 / 脚本调用，启动和退出都更快。

use std::sync::Arc;
use voicekey_core::{audio, Engine};
use voicekey_wtlocal::LocalEngine;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 || args[1] != "--asr" {
        eprintln!("usage: voicekey-asr --asr <channel> <wav>");
        std::process::exit(2);
    }
    let channel = args[2].clone();
    let wav = args[3].clone();

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    };

    let result = rt.block_on(async move {
        let engine: Arc<dyn Engine> = match channel.as_str() {
            "wetype" => Arc::new(voicekey_core::WeTypeEngine::default()),
            "qwen" => Arc::new(voicekey_core::QwenEngine::default()),
            "baidu" => Arc::new(voicekey_core::BaiduEngine),
            "sogou" => Arc::new(voicekey_core::SogouEngine),
            "iflytek" => Arc::new(voicekey_core::IflyEngine),
            "chatterfly" => Arc::new(voicekey_core::ChatterflyEngine),
            "wetypeoffline" => Arc::new(LocalEngine {
                dir: voicekey_core::util::data_dir().join("wtoffline"),
            }),
            _ => Arc::new(voicekey_core::DoubaoEngine),
        };
        let audio = audio::file_frames(&wav, false)?;
        engine.run(audio, Box::new(|_| {})).await
    });

    match result {
        Ok(text) => {
            let terms = voicekey_core::terms::Terms::load(&voicekey_core::terms::default_path());
            println!("{}", terms.apply(&text));
        }
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }
}
