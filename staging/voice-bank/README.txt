短剧工坊音色包

1. 确认本目录有 manifest.json 与 wav/*.wav
2. 上传 staging/voice-bank-<version>.zip 与 version.json 到魔搭
   默认路径：datasets/NanoModel/ffmpeg/voices/
3. 客户端用 FLOWY_VOICE_BANK_VERSION_URL 覆盖地址（可选）
4. 升版本：改 version、换 zip，客户端会下载并替换 {data_dir}/voice-bank/current/
