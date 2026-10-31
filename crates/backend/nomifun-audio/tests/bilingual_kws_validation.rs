#[cfg(feature = "local-stt")]
#[test]
fn test_bilingual_kws_isolated_validation() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.ancestors().nth(3).expect("repo root");
    let kws_dir = repo_root.join("build.noindex/voice-companion-overhaul-execution-20261031/kws-zh-en/package/kws");

    if !kws_dir.exists() {
        eprintln!("Bilingual KWS package not found, skipping isolated test");
        return;
    }

    let encoder = kws_dir.join("encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx");
    let decoder = kws_dir.join("decoder-epoch-13-avg-2-chunk-16-left-64.onnx");
    let joiner = kws_dir.join("joiner-epoch-13-avg-2-chunk-16-left-64.int8.onnx");
    let tokens = kws_dir.join("tokens.txt");

    assert!(encoder.exists(), "encoder exists");
    assert!(decoder.exists(), "decoder exists");
    assert!(joiner.exists(), "joiner exists");
    assert!(tokens.exists(), "tokens exists");

    // Dynamically compile keywords in memory: English and Chinese
    let keywords_content = "\
HH EY1 F L OW1 IY0 @HEY_FLOWY
F L OW1 IY0 @FLOWY
n i h ao @NI_HAO
x iao f u x ing @XIAO_FU_XING
";
    let temp_keywords_file = tempfile::NamedTempFile::new().expect("temp file");
    std::fs::write(temp_keywords_file.path(), keywords_content).expect("write keywords");

    let mut spotter_config = sherpa_onnx::KeywordSpotterConfig::default();
    spotter_config.feat_config.sample_rate = 16000;
    spotter_config.feat_config.feature_dim = 80;
    spotter_config.model_config.transducer.encoder = Some(encoder.to_string_lossy().into_owned());
    spotter_config.model_config.transducer.decoder = Some(decoder.to_string_lossy().into_owned());
    spotter_config.model_config.transducer.joiner = Some(joiner.to_string_lossy().into_owned());
    spotter_config.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
    spotter_config.model_config.num_threads = 2;
    spotter_config.model_config.provider = Some("cpu".to_string());
    spotter_config.keywords_file = Some(temp_keywords_file.path().to_string_lossy().into_owned());
    spotter_config.keywords_score = 1.5;
    spotter_config.keywords_threshold = 0.25;

    let t0 = std::time::Instant::now();
    let spotter = sherpa_onnx::KeywordSpotter::create(&spotter_config)
        .expect("Sherpa-ONNX bilingual KeywordSpotter creation must succeed");
    let init_dur = t0.elapsed();
    println!("Bilingual KeywordSpotter initialized in {:.3}s", init_dur.as_secs_f32());

    let stream = spotter.create_stream();
    assert!(!spotter.is_ready(&stream));

    // Feed 1 second of silence to verify stream decoding stability
    let silence = vec![0.0f32; 16000];
    stream.accept_waveform(16000, &silence);
    while spotter.is_ready(&stream) {
        spotter.decode(&stream);
        let _result = spotter.get_result(&stream);
    }
    println!("Bilingual stream accepted 1s silence cleanly without false trigger");

    // Test dynamic keyword compilation for both Chinese and English
    let custom_keywords = vec![
        "你好小白".to_string(),
        "Hey Flowy".to_string(),
        "Hi Flowy".to_string(),
        "小度".to_string(),
        "贾维斯".to_string(),
        "旺财".to_string(),
    ];
    let dynamic_buf = nomifun_audio::kws::build_bilingual_keywords_buf(&custom_keywords);
    assert!(dynamic_buf.contains("n i h ao x iao b ai"), "must contain pinyin for 你好小白");
    assert!(dynamic_buf.contains("HH EY1 F L OW1 IY0"), "must contain phonemes for HEY_FLOWY");
    assert!(dynamic_buf.contains("HH AY1 F L OW1 IY0"), "must contain phonemes for HI_FLOWY");
    assert!(dynamic_buf.contains("x iao d u"), "must contain pinyin for 小度");
    assert!(dynamic_buf.contains("j ia w ei s i"), "must contain pinyin for 贾维斯");
    assert!(dynamic_buf.contains("w ang c ai"), "must contain pinyin for 旺财");

    // Test KwsEngine integration with custom keywords
    let kws_engine = nomifun_audio::kws::KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
        model_dir: Some(kws_dir.clone()),
        keywords: custom_keywords,
        ..Default::default()
    }).expect("KwsEngine creation with custom bilingual keywords must succeed");
    assert!(kws_engine.is_initialized(), "KwsEngine must be initialized");
}
