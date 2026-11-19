package app.lunta.listen

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.MediaMuxer
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** A rendered narration (16-bit mono WAV) as AAC in an .m4a, as the Mac writes it (afconvert, 48 kbit/s). */
object AacEncoder {
    private const val BITRATE = 48_000

    fun encodeWav(wav: File, out: File) {
        RandomAccessFile(wav, "r").use { f ->
            val header = ByteArray(44)
            f.readFully(header)
            val h = ByteBuffer.wrap(header).order(ByteOrder.LITTLE_ENDIAN)
            require(String(header, 0, 4) == "RIFF" && String(header, 8, 4) == "WAVE") { "not a WAV file" }
            val channels = h.getShort(22).toInt()
            val rate = h.getInt(24)
            val dataLen = h.getInt(40).toLong().coerceAtMost(f.length() - 44)

            val format = MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_AAC, rate, channels).apply {
                setInteger(MediaFormat.KEY_AAC_PROFILE, MediaCodecInfo.CodecProfileLevel.AACObjectLC)
                setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
                setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 16 * 1024)
            }
            val codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_AUDIO_AAC)
            out.delete()
            val muxer = MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            try {
                codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                codec.start()
                val info = MediaCodec.BufferInfo()
                val chunk = ByteArray(8 * 1024)
                var read = 0L
                var inputDone = false
                var track = -1
                val bytesPerSecond = rate.toLong() * channels * 2
                while (true) {
                    if (!inputDone) {
                        val i = codec.dequeueInputBuffer(10_000)
                        if (i >= 0) {
                            val buf = codec.getInputBuffer(i)!!
                            buf.clear()
                            val want = minOf(chunk.size.toLong(), dataLen - read, buf.capacity().toLong()).toInt()
                            val pts = read * 1_000_000 / bytesPerSecond
                            if (want <= 0) {
                                codec.queueInputBuffer(i, 0, 0, pts, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                                inputDone = true
                            } else {
                                f.readFully(chunk, 0, want)
                                buf.put(chunk, 0, want)
                                codec.queueInputBuffer(i, 0, want, pts, 0)
                                read += want
                            }
                        }
                    }
                    val o = codec.dequeueOutputBuffer(info, 10_000)
                    when {
                        o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                            track = muxer.addTrack(codec.outputFormat)
                            muxer.start()
                        }
                        o >= 0 -> {
                            val buf = codec.getOutputBuffer(o)!!
                            if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) info.size = 0
                            if (info.size > 0 && track >= 0) {
                                buf.position(info.offset)
                                buf.limit(info.offset + info.size)
                                muxer.writeSampleData(track, buf, info)
                            }
                            codec.releaseOutputBuffer(o, false)
                            if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
                        }
                    }
                }
            } finally {
                runCatching { codec.stop() }
                codec.release()
                runCatching { muxer.stop() }
                muxer.release()
            }
        }
    }
}
