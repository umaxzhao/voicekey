//! 无界面识别入口：`voicekey --asr <channel> <wav>`。
//!
//! 供 Linux IBus 引擎 / 脚本调用：读入 16kHz 单声道 WAV，走和 GUI 相同的识别渠道，
//! 最终文本写到 stdout（进度和错误写 stderr），退出码 0/1。

use std::sync::Arc;
use voicekey_core::{audio, Engine};

pub fn main() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 || args[1] != "--asr" {
        return None;
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
            return Some(1);
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
            "wetypeoffline" => crate::wtoffline::engine(),
            _ => Arc::new(voicekey_core::DoubaoEngine),
        };
        // 文件识别不需要按实时速度喂音频；false 可显著降低结束后等待。
        let audio = audio::file_frames(&wav, false)?;
        engine.run(audio, Box::new(|_| {})).await
    });

    match result {
        Ok(text) => {
            let terms = voicekey_core::terms::Terms::load(&voicekey_core::terms::default_path());
            println!("{}", terms.apply(&text));
            Some(0)
        }
        Err(e) => {
            eprintln!("{e:#}");
            Some(1)
        }
    }
}
