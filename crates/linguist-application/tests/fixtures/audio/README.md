These fixtures are newly generated 0.1-second 440 Hz tones at 8, 16 and 44.1 kHz, with mono and stereo coverage.
They contain no third-party recordings. FFmpeg is required only to regenerate
the fixtures, not to build, run or test the CLI.

Generation commands (run only when deliberately replacing the fixtures):

```sh
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=16000:duration=0.1 -map_metadata -1 -c:a libmp3lame -b:a 32k -write_xing 0 tone.mp3
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=16000:duration=0.1 -map_metadata -1 -c:a libvorbis tone.ogg
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=44100:duration=0.1 -ac 2 -map_metadata -1 -c:a libmp3lame -q:a 5 -write_xing 0 tone-vbr.mp3
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=8000:duration=0.1 -map_metadata -1 -c:a libmp3lame -b:a 8k -write_xing 0 tone-low-rate.mp3
```

The Ogg encoder may choose a different stream serial number on regeneration.
Tests inspect decoded properties rather than requiring a byte-identical encode.
