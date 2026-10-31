#[cfg(feature = "local-tts")]
#[test]
fn test_real_matcha_direct_synthesis() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.ancestors().nth(3).expect("repo root");
    let evidence_dir = repo_root.join("build.noindex/voice-companion-overhaul-execution-20261031");
    let zh_dir = evidence_dir.join("matcha-icefall-zh-baker");
    let en_dir = evidence_dir.join("matcha-icefall-en_US-ljspeech");
    let vocoder = evidence_dir.join("vocos-22khz-univ.onnx");

    if !zh_dir.exists() || !en_dir.exists() || !vocoder.exists() {
        eprintln!("Matcha models not found in build.noindex, skipping real test");
        return;
    }

    // 1. Chinese Matcha Engine
    let zh_acoustic = zh_dir.join("model-steps-3.onnx").to_string_lossy().into_owned();
    let zh_vocoder = vocoder.to_string_lossy().into_owned();
    let zh_lexicon = zh_dir.join("lexicon.txt").to_string_lossy().into_owned();
    let zh_tokens = zh_dir.join("tokens.txt").to_string_lossy().into_owned();
    let zh_dict = zh_dir.join("dict").to_string_lossy().into_owned();
    let zh_rules = format!(
        "{},{},{}",
        zh_dir.join("date.fst").to_string_lossy(),
        zh_dir.join("number.fst").to_string_lossy(),
        zh_dir.join("phone.fst").to_string_lossy()
    );

    let zh_config = sherpa_onnx::OfflineTtsConfig {
        model: sherpa_onnx::OfflineTtsModelConfig {
            matcha: sherpa_onnx::OfflineTtsMatchaModelConfig {
                acoustic_model: Some(zh_acoustic),
                vocoder: Some(zh_vocoder.clone()),
                lexicon: Some(zh_lexicon),
                tokens: Some(zh_tokens),
                data_dir: None,
                noise_scale: 0.667,
                length_scale: 1.0,
                dict_dir: Some(zh_dict),
            },
            num_threads: 2,
            debug: false,
            provider: Some("cpu".into()),
            ..Default::default()
        },
        rule_fsts: Some(zh_rules),
        rule_fars: None,
        max_num_sentences: 1,
        silence_scale: 0.2,
    };

    println!("Initializing Chinese Matcha engine...");
    let t_init = std::time::Instant::now();
    let zh_engine = sherpa_onnx::OfflineTts::create(&zh_config)
        .expect("Failed to create Chinese Matcha OfflineTts");
    println!("Chinese Matcha initialized in {:.2}s, sample_rate={}", t_init.elapsed().as_secs_f32(), zh_engine.sample_rate());

    let gen_config = sherpa_onnx::GenerationConfig {
        speed: 1.0,
        sid: 0,
        ..Default::default()
    };

    let text_zh = "你好，我是你的语音伴侣。";
    let t_gen = std::time::Instant::now();
    let audio_zh = zh_engine
        .generate_with_config::<fn(&[f32], f32) -> bool>(text_zh, &gen_config, None)
        .expect("Chinese generation failed");
    let dur_gen = t_gen.elapsed();
    let samples_zh = audio_zh.samples();
    let audio_sec = samples_zh.len() as f32 / audio_zh.sample_rate() as f32;
    let rtf = dur_gen.as_secs_f32() / audio_sec;
    println!("Chinese synthesis: '{}' -> {} samples ({:.2}s audio) in {:.2}s, RTF={:.3}",
        text_zh, samples_zh.len(), audio_sec, dur_gen.as_secs_f32(), rtf
    );
    assert!(samples_zh.len() > 1000);
    assert_eq!(audio_zh.sample_rate(), 22050);

    // 2. English Matcha Engine
    let en_acoustic = en_dir.join("model-steps-3.onnx").to_string_lossy().into_owned();
    let en_tokens = en_dir.join("tokens.txt").to_string_lossy().into_owned();
    let en_data = en_dir.join("espeak-ng-data").to_string_lossy().into_owned();

    let en_config = sherpa_onnx::OfflineTtsConfig {
        model: sherpa_onnx::OfflineTtsModelConfig {
            matcha: sherpa_onnx::OfflineTtsMatchaModelConfig {
                acoustic_model: Some(en_acoustic),
                vocoder: Some(zh_vocoder),
                lexicon: None,
                tokens: Some(en_tokens),
                data_dir: Some(en_data),
                noise_scale: 0.667,
                length_scale: 1.0,
                dict_dir: None,
            },
            num_threads: 2,
            debug: false,
            provider: Some("cpu".into()),
            ..Default::default()
        },
        rule_fsts: None,
        rule_fars: None,
        max_num_sentences: 1,
        silence_scale: 0.2,
    };

    println!("Initializing English Matcha engine...");
    let t_init_en = std::time::Instant::now();
    let en_engine = sherpa_onnx::OfflineTts::create(&en_config)
        .expect("Failed to create English Matcha OfflineTts");
    println!("English Matcha initialized in {:.2}s, sample_rate={}", t_init_en.elapsed().as_secs_f32(), en_engine.sample_rate());

    let text_en = "Hello, I am your voice companion.";
    let t_gen_en = std::time::Instant::now();
    let audio_en = en_engine
        .generate_with_config::<fn(&[f32], f32) -> bool>(text_en, &gen_config, None)
        .expect("English generation failed");
    let dur_gen_en = t_gen_en.elapsed();
    let samples_en = audio_en.samples();
    let audio_sec_en = samples_en.len() as f32 / audio_en.sample_rate() as f32;
    let rtf_en = dur_gen_en.as_secs_f32() / audio_sec_en;
    println!("English synthesis: '{}' -> {} samples ({:.2}s audio) in {:.2}s, RTF={:.3}",
        text_en, samples_en.len(), audio_sec_en, dur_gen_en.as_secs_f32(), rtf_en
    );
    assert!(samples_en.len() > 1000);
    assert_eq!(audio_en.sample_rate(), 22050);

    // 3. Bilingual Mixed Synthesis Test
    let mixed_text = "好的，我正在为你使用 Docker 构建镜像。";
    println!("Testing bilingual mixed synthesis for: '{}'", mixed_text);

    // Segment into Zh and En
    let segments = vec![
        ("zh", "好的，我正在为你使用 "),
        ("en", "Docker"),
        ("zh", " 构建镜像。"),
    ];

    let t_mix = std::time::Instant::now();
    let mut mixed_samples = Vec::new();
    let silence_gap = vec![0.0f32; (22050.0 * 0.08) as usize]; // 80ms silence

    for (i, (lang, chunk)) in segments.iter().enumerate() {
        if i > 0 {
            mixed_samples.extend_from_slice(&silence_gap);
        }
        let samples = if *lang == "zh" {
            let a = zh_engine.generate_with_config::<fn(&[f32], f32) -> bool>(chunk, &gen_config, None).expect("zh chunk");
            a.samples().to_vec()
        } else {
            let a = en_engine.generate_with_config::<fn(&[f32], f32) -> bool>(chunk, &gen_config, None).expect("en chunk");
            a.samples().to_vec()
        };
        mixed_samples.extend_from_slice(&samples);
    }
    let dur_mix = t_mix.elapsed();
    let total_sec = mixed_samples.len() as f32 / 22050.0;
    let rtf_mix = dur_mix.as_secs_f32() / total_sec;
    println!("Bilingual mixed synthesis: {} samples ({:.2}s audio) in {:.2}s, RTF={:.3}",
        mixed_samples.len(), total_sec, dur_mix.as_secs_f32(), rtf_mix
    );
    assert!(mixed_samples.len() > 10000);
}
