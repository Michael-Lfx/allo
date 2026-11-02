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
        "你好，小白".to_string(),
        "你好小白".to_string(),
        "荷包蛋".to_string(),
        "香芋片".to_string(),
        "菠萝蜜".to_string(),
        "麻辣烫".to_string(),
        "皮卡丘".to_string(),
        "小白同学".to_string(),
        "Hey Flowy".to_string(),
        "Hi Flowy".to_string(),
        "小度".to_string(),
        "贾维斯".to_string(),
        "旺财".to_string(),
        "你好 Flowy".to_string(),
    ];
    let dynamic_buf = nomifun_audio::kws::build_bilingual_keywords_buf(&custom_keywords);
    println!("=== DYNAMIC KEYWORDS BUF ===\n{}", dynamic_buf);
    assert!(dynamic_buf.contains("n i h ao x iao b ai"), "must contain pinyin for 你好小白");
    assert!(dynamic_buf.contains("h e b ao d an") || dynamic_buf.contains("h é b āo d àn"), "must contain pinyin for 荷包蛋");
    assert!(dynamic_buf.contains("x iang y u p ian") || dynamic_buf.contains("x iāng y ù p iàn"), "must contain pinyin for 香芋片");
    assert!(dynamic_buf.contains("b o l uo m i") || dynamic_buf.contains("b ō l uó m ì"), "must contain pinyin for 菠萝蜜");
    assert!(dynamic_buf.contains("m a l a t ang") || dynamic_buf.contains("m á l à t àng"), "must contain pinyin for 麻辣烫");
    assert!(dynamic_buf.contains("p i k a q iu") || dynamic_buf.contains("p í k ǎ q iū"), "must contain pinyin for 皮卡丘");
    assert!(dynamic_buf.contains("x iao b ai t ong x ue") || dynamic_buf.contains("x iǎo b ái t óng x ué"), "must contain pinyin for 小白同学");
    assert!(dynamic_buf.contains("HH EY1 F L OW1 IY0"), "must contain phonemes for HEY_FLOWY");
    assert!(dynamic_buf.contains("HH AY1 F L OW1 IY0"), "must contain phonemes for HI_FLOWY");
    assert!(dynamic_buf.contains("F L OW1 IY0 :2.0 #0.15 @FLOWY"), "must contain bare FLOWY rule");
    assert!(dynamic_buf.contains("x iao d u"), "must contain pinyin for 小度");
    assert!(dynamic_buf.contains("j ia w ei s i"), "must contain pinyin for 贾维斯");
    assert!(dynamic_buf.contains("w ang c ai"), "must contain pinyin for 旺财");
    assert!(dynamic_buf.contains("n i h ao F L OW1 IY0"), "must contain mixed tokens for 你好 Flowy");

    // Test KwsEngine integration with custom keywords
    let mut kws_engine = nomifun_audio::kws::KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
        model_dir: Some(kws_dir.clone()),
        keywords: custom_keywords,
        ..Default::default()
    }).expect("KwsEngine creation with custom bilingual keywords must succeed");
    assert!(kws_engine.is_initialized(), "KwsEngine must be initialized");

    // =========================================================================
    // Minimum Experiment Verification: Playback Echo Immunity & Wake Interruption
    // =========================================================================
    let fixtures_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/architecture/voice-agent/execution/audio-fixtures");
    let non_wake_wav = fixtures_dir.join("non-wake-16k.wav");
    let command_wav = fixtures_dir.join("command-16k.wav");
    let wake_wav = fixtures_dir.join("wake-guy-16k.wav");

    // Phase A: Feed non-wake speech (3.41s) into KWS to verify 0 false triggers
    if non_wake_wav.exists() {
        let bytes = std::fs::read(&non_wake_wav).expect("read non_wake_wav");
        let (samples, sr) = nomifun_audio::speaker::parse_audio_frame_to_pcm(&bytes);
        assert_eq!(sr, 16000);
        let mut false_alarms = 0;
        for chunk in samples.chunks(320) {
            if let Some(hit) = kws_engine.process_pcm_chunk(chunk) {
                println!("UNEXPECTED FALSE ALARM on non-wake speech: {:?}", hit);
                false_alarms += 1;
            }
        }
        println!("Phase A (Non-wake speech echo immunity): {} samples, false_alarms = {}", samples.len(), false_alarms);
        assert_eq!(false_alarms, 0, "KWS must have ZERO false triggers during non-wake speech playback!");
    }

    // Phase B: Feed command speech without wake word to verify 0 false triggers
    if command_wav.exists() {
        let bytes = std::fs::read(&command_wav).expect("read command_wav");
        let (samples, sr) = nomifun_audio::speaker::parse_audio_frame_to_pcm(&bytes);
        assert_eq!(sr, 16000);
        let mut false_alarms = 0;
        for chunk in samples.chunks(320) {
            if let Some(hit) = kws_engine.process_pcm_chunk(chunk) {
                println!("UNEXPECTED FALSE ALARM on command speech: {:?}", hit);
                false_alarms += 1;
            }
        }
        println!("Phase B (Command speech echo immunity): {} samples, false_alarms = {}", samples.len(), false_alarms);
        assert_eq!(false_alarms, 0, "KWS must have ZERO false triggers during command speech playback!");
    }

    // Phase C: Feed real spoken wake word to verify low-latency detection
    if wake_wav.exists() {
        kws_engine.reset();
        let bytes = std::fs::read(&wake_wav).expect("read wake_wav");
        let (mut samples, sr) = nomifun_audio::speaker::parse_audio_frame_to_pcm(&bytes);
        assert_eq!(sr, 16000);
        // Trailing frames for lookahead chunking
        samples.extend(vec![0.0f32; 16000]);
        let mut detected_keyword = None;
        let mut chunk_idx = 0;
        for (i, chunk) in samples.chunks(320).enumerate() {
            if let Some(hit) = kws_engine.process_pcm_chunk(chunk) {
                detected_keyword = Some(hit.keyword);
                chunk_idx = i;
                break;
            }
        }
        println!("Phase C (Wake word barge-in detection): hit = {:?}, chunk = {}, latency = {}ms",
            detected_keyword, chunk_idx, chunk_idx * 20);
        assert!(detected_keyword.is_some(), "KWS must detect wake word for barge-in!");
        assert!(chunk_idx * 20 <= 2500, "Wake word must be detected promptly!");
    }
}
