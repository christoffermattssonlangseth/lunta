package app.lunta.listen

import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaPlayer
import android.media.PlaybackParams
import android.os.Bundle
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import org.json.JSONObject
import uniffi.lunta_core.*
import java.io.File
import java.util.Locale
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

/**
 * Listen: the paper read aloud, as the Apple apps do it (apple/Shared/Narrator.swift).
 *
 * The reading view's narration items (narration.js) are read one after the other; the one being read
 * is highlighted and kept in view while its reader is open. Three ways to read:
 *  - a narration made on the Mac or here ("Make Natural Narration": narration.m4a + narration.json),
 *    played with its timing map;
 *  - the natural voice (Kokoro-82M through the core, core/src/tts): sentence pieces made a few ahead
 *    of what plays and streamed into one AudioTrack, so there are no gaps and the highlight follows
 *    the audio frame by frame;
 *  - the phone's own voice (TextToSpeech) until the natural voice is downloaded.
 * Playback goes on in the background: [ListenService] holds the media session (notification, lock
 * screen, headset buttons) and keeps the app running as a media playback service.
 */
object Narrator {
    enum class Mode { Natural, System, Recorded }

    /** One narration item as it will be said: its pieces (sentences, joined up to a length). */
    data class Item(val id: String, val kind: String, val text: String, val pieces: List<String>)

    // ---- what the UI shows (Compose state; changed on the main thread only) ----
    var items by mutableStateOf<List<Item>>(emptyList()); private set
    var index by mutableIntStateOf(0); private set
    var playing by mutableStateOf(false); private set
    var waiting by mutableStateOf(false); private set
    var mode by mutableStateOf(Mode.System); private set
    var title by mutableStateOf(""); private set
    var paperId by mutableStateOf(""); private set
    var speed by mutableFloatStateOf(1f); private set
    var voice by mutableStateOf("af_heart"); private set
    /** A line under the player: "offer" (system voice; download the natural one), "novoice", or a message. */
    var note by mutableStateOf<String?>(null)
    /** The paper whose reader shows the player now (the library's Now Playing bar steps aside for it). */
    var readerPaper by mutableStateOf<String?>(null); private set
    var voiceStatus by mutableStateOf<VoiceStatus?>(null); private set
    var download by mutableStateOf<Pair<Long, Long>?>(null); private set
    /** Make Natural Narration: (paper, items done, items) while it runs. */
    var rendering by mutableStateOf<Triple<String, Int, Int>?>(null); private set

    val speeds = listOf(0.8f, 1.0f, 1.15f, 1.3f, 1.5f, 1.75f, 2.0f)
    val voices: List<VoiceInfo> by lazy { ttsVoices() }

    private lateinit var app: Context
    private lateinit var natural: NaturalVoice
    private val main = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    /** The voice makes one piece at a time: one thread for it. */
    private val synth = Executors.newSingleThreadExecutor { r -> Thread(r, "lunta-voice").apply { priority = Thread.NORM_PRIORITY + 1 } }.asCoroutineDispatcher()
    private var highlight: ((String) -> Unit)? = null
    private var setListening: ((Boolean) -> Unit)? = null
    internal var player: NarrationPlayer? = null
    private var controller: ListenableFuture<MediaController>? = null

    fun init(ctx: Context) {
        if (::app.isInitialized) return
        app = ctx.applicationContext
        val p = prefs()
        speed = p.getFloat("narrationSpeed", 1f)
        voice = p.getString("neuralVoice", "af_heart")!!
        natural = NaturalVoice(File(app.noBackupFilesDir, "voice").path)
        // sherpa-onnx comes with the app on Android (scripts/fetch-sherpa-onnx-android.sh)
        natural.setLibraryDir(app.applicationInfo.nativeLibraryDir)
        main.launch { voiceStatus = withContext(Dispatchers.IO) { natural.status() } }
    }

    private fun prefs() = app.getSharedPreferences("listen", Context.MODE_PRIVATE)

    // ---- starting ----

    /**
     * Starts reading a paper. [pageItems]: luntaNarration(false) from its reading view; [fromId]: the
     * paragraph the reader is looking at; [recorded]: narration.m4a and narration.json, when it has them.
     */
    fun start(id: String, paperTitle: String, pageItems: List<NarrationSegment>, fromId: String, recorded: Pair<String, String>?) {
        stop()
        paperId = id
        title = paperTitle
        note = null
        main.launch {
            if (recorded != null && loadRecorded(recorded, pageItems)) {
                mode = Mode.Recorded
            } else {
                items = withContext(Dispatchers.Default) {
                    pageItems.mapNotNull { s ->
                        val said = ttsSpoken(s.text, s.kind)
                        if (said.isBlank()) null else Item(s.id, s.kind, s.text, ttsChunks(said, 300u))
                    }
                }
                val st = withContext(Dispatchers.IO) { natural.status() }
                voiceStatus = st
                mode = if (st.installed) Mode.Natural else Mode.System
                if (mode == Mode.System) {
                    note = if (systemReady()) "offer" else "novoice"
                } else {
                    launch(synth) { runCatching { natural.prepare(voice) } }
                }
            }
            if (items.isEmpty()) { note = "Nothing to read aloud in this paper."; return@launch }
            setListening?.invoke(true)
            val from = items.indexOfFirst { it.id == fromId }.coerceAtLeast(0)
            if (note == "novoice") { index = from; refresh(); return@launch }
            play(from)
        }
    }

    /** The reader of [id] is on screen: the page follows the voice. False if this paper isn't being read. */
    fun attach(id: String, onHighlight: (String) -> Unit, onListening: (Boolean) -> Unit): Boolean {
        highlight = onHighlight
        setListening = onListening
        readerPaper = id
        if (items.isEmpty() || id != paperId) return false
        onListening(true)
        items.getOrNull(index)?.let { onHighlight(it.id) }
        return true
    }

    /** The reader closes; narration goes on (Now Playing in the library). */
    fun detach(id: String) {
        if (readerPaper != id) return
        readerPaper = null
        highlight = null
        setListening = null
    }

    // ---- control ----

    fun play(i: Int = index) {
        if (items.isEmpty()) return
        index = i.coerceIn(0, items.lastIndex)
        playing = true
        if (!focus()) { playing = false; return }
        ensureSession()
        highlight?.invoke(items[index].id)
        when (mode) {
            Mode.Natural -> startNatural(index)
            Mode.System -> { piece = 0; speakSystem() }
            Mode.Recorded -> recorder?.let { mp ->
                mp.seekTo((times[index].first * 1000).toInt())
                setRate(mp)
                mp.start()
                startTicker()
            }
        }
        refresh()
    }

    fun playFromId(id: String) {
        val i = items.indexOfFirst { it.id == id }
        if (i >= 0) play(i)
    }

    fun toggle() { if (playing) pause() else resume() }

    fun resume() {
        if (items.isEmpty()) return
        if (mode == Mode.System && note == "novoice") return
        when (mode) {
            Mode.Natural -> {
                val t = track
                if (t != null && pipeline?.isActive == true) {
                    if (!focus()) return
                    playing = true
                    runCatching { t.play() }
                    refresh()
                } else play(index)
            }
            Mode.Recorded -> {
                val mp = recorder ?: return
                if (!focus()) return
                playing = true
                setRate(mp)
                mp.start()
                startTicker()
                refresh()
            }
            Mode.System -> play(index)     // the phone's voice starts the paragraph over
        }
    }

    fun pause() {
        playing = false
        waiting = false
        when (mode) {
            Mode.Natural -> runCatching { track?.pause() }
            Mode.Recorded -> runCatching { recorder?.pause() }
            Mode.System -> { utterance = null; tts?.stop() }
        }
        unfocus(keep = true)
        refresh()
    }

    fun stop() {
        playing = false
        waiting = false
        stopNatural()
        utterance = null
        tts?.stop()
        recorder?.release(); recorder = null
        ticker?.cancel(); ticker = null
        setListening?.invoke(false)
        items = emptyList()
        times = emptyList()
        index = 0
        unfocus(keep = false)
        refresh()
        controller?.let { MediaController.releaseFuture(it) }
        controller = null
    }

    fun next() { if (index + 1 < items.size) play(index + 1) }
    fun previous() = play((index - 1).coerceAtLeast(0))
    fun nextSection() {
        val i = (index + 1 until items.size).firstOrNull { items[it].kind == "heading" } ?: return
        play(i)
    }

    fun sectionAt(i: Int): String {
        for (j in i.coerceAtMost(items.lastIndex) downTo 0) if (items[j].kind == "heading") return items[j].text
        return ""
    }
    val section: String get() = sectionAt(index)
    val progress: Float get() = if (mode == Mode.Recorded) recorder?.let { mp -> if (mp.duration > 0) mp.currentPosition.toFloat() / mp.duration else 0f } ?: 0f
        else if (items.isEmpty()) 0f else index.toFloat() / items.size

    fun changeSpeed(s: Float) {
        speed = s
        prefs().edit { putFloat("narrationSpeed", s) }
        when (mode) {
            Mode.Natural -> track?.let { setRate(it) }
            Mode.Recorded -> recorder?.let { if (playing) setRate(it) }
            Mode.System -> if (playing) speakSystem()
        }
        refresh()
    }

    fun changeVoice(id: String) {
        voice = id
        prefs().edit { putString("neuralVoice", id) }
        if (mode == Mode.Natural && items.isNotEmpty() && playing) play(index)
    }

    // ---- the natural voice: pieces made ahead, streamed into one AudioTrack ----

    private const val AHEAD = 3
    private var pipeline: Job? = null
    @Volatile private var track: AudioTrack? = null
    /** Where each item starts in the stream (frames), as the pieces are written. */
    private class Mark(val frame: Long, val item: Int)
    private val marks = CopyOnWriteArrayList<Mark>()
    private val written = AtomicLong(0)
    @Volatile private var writerDone = false
    private class Clip(val item: Int, val pcm: ByteArray, val before: Float, val after: Float)

    private fun pauses(kind: String): Pair<Float, Float> = when (kind) {
        "heading" -> 0.55f to 0.45f
        "caption" -> 0.2f to 0.4f
        else -> 0f to 0.35f
    }

    private fun startNatural(from: Int) {
        stopNatural()
        marks.clear(); written.set(0); writerDone = false
        waiting = true
        val list = items
        val v = voice
        val clips = Channel<Clip>(AHEAD)
        pipeline = main.launch {
            // the voice, in order from here, a few pieces ahead of the speaker
            val producer = launch(synth) {
                try {
                    natural.prepare(v)
                    for (a in from until list.size) {
                        val it = list[a]
                        val (before, after) = pauses(it.kind)
                        it.pieces.forEachIndexed { b, text ->
                            ensureActive()
                            val pcm = natural.synthesizePcm(text, v, 1f)
                            clips.send(Clip(a, pcm, if (b == 0) before else 0f, if (b == it.pieces.lastIndex) after else 0.08f))
                        }
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    withContext(Dispatchers.Main) { note = "The natural voice failed: ${e.message}" }
                } finally {
                    clips.close()
                }
            }
            val rate = runCatching { natural.sampleRate().toInt() }.getOrDefault(24_000)
            val t = AudioTrack.Builder()
                .setAudioAttributes(attributes)
                .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(rate).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setBufferSizeInBytes(maxOf(AudioTrack.getMinBufferSize(rate, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT) * 4, rate))
                .build()
            track = t
            setRate(t)
            // the speaker: writes the pieces into the stream as they come (blocks while it is full)
            launch(Dispatchers.IO) {
                var started = false
                try {
                    for (c in clips) {
                        if (!isActive) break
                        marks.add(Mark(written.get(), c.item))
                        writeSilence(t, c.before, rate)
                        writeAll(t, c.pcm)
                        writeSilence(t, c.after, rate)
                        if (!started) {
                            started = true
                            withContext(Dispatchers.Main) { if (playing && track === t) runCatching { t.play() } }
                        }
                    }
                } finally {
                    writerDone = true
                }
            }
            // the highlight follows what is heard
            while (isActive) {
                delay(120)
                if (track !== t) break
                val head = t.playbackHeadPosition.toLong() and 0xffffffffL
                val m = marks.lastOrNull { it.frame <= head }
                if (m != null && m.item != index) {
                    index = m.item
                    highlight?.invoke(list[m.item].id)
                    refresh()
                }
                val done = writerDone && producer.isCompleted
                val wait = playing && head >= written.get() && !done
                if (wait != waiting) { waiting = wait; refresh() }
                if (done && head >= written.get() && playing) {
                    // the end of the paper
                    playing = false
                    waiting = false
                    unfocus(keep = false)
                    refresh()
                    break
                }
            }
        }
    }

    private fun writeAll(t: AudioTrack, pcm: ByteArray) {
        var off = 0
        while (off < pcm.size) {
            val n = t.write(pcm, off, pcm.size - off)
            if (n <= 0) throw CancellationException("audio track closed")
            off += n
        }
        written.addAndGet((pcm.size / 2).toLong())
    }

    private fun writeSilence(t: AudioTrack, secs: Float, rate: Int) {
        if (secs <= 0f) return
        writeAll(t, ByteArray((secs * rate).toInt() * 2))
    }

    private fun stopNatural() {
        pipeline?.cancel()
        pipeline = null
        track?.let { t ->
            track = null
            // unblocks the writer; released once it has let go
            runCatching { t.pause(); t.flush(); t.stop() }
            main.launch(Dispatchers.IO) { delay(500); runCatching { t.release() } }
        }
    }

    private val attributes: AudioAttributes = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
        .build()

    private fun setRate(t: AudioTrack) = runCatching { t.playbackParams = PlaybackParams().setSpeed(speed).setPitch(1f) }
    private fun setRate(mp: MediaPlayer) = runCatching { mp.playbackParams = mp.playbackParams.setSpeed(speed) }

    // ---- the phone's own voice ----

    private var tts: TextToSpeech? = null
    private var ttsState = 0          // 0 starting, 1 ready, -1 none
    private var piece = 0
    private var utterance: String? = null
    private var utteranceSeq = 0

    /** Starts the phone's voice; false if it has none. */
    private suspend fun systemReady(): Boolean {
        if (tts == null) {
            ttsState = 0
            tts = TextToSpeech(app) { status ->
                ttsState = if (status == TextToSpeech.SUCCESS) 1 else -1
                if (ttsState == 1) tts?.let { t ->
                    val ok = t.isLanguageAvailable(Locale.US) >= TextToSpeech.LANG_AVAILABLE
                    t.language = if (ok) Locale.US else Locale.ENGLISH
                    t.setAudioAttributes(attributes)
                    t.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
                        override fun onStart(id: String?) {}
                        override fun onDone(id: String?) { main.launch { if (id == utterance && playing) advanceSystem() } }
                        @Deprecated("Deprecated in Java")
                        override fun onError(id: String?) { main.launch { if (id == utterance && playing) advanceSystem() } }
                    })
                }
            }
        }
        repeat(50) { if (ttsState != 0) return ttsState == 1; delay(100) }
        return ttsState == 1
    }

    private fun speakSystem() {
        val it = items.getOrNull(index) ?: return
        val t = tts ?: return
        t.setSpeechRate(speed)
        val id = "u${++utteranceSeq}"
        utterance = id
        t.speak(it.pieces.getOrElse(piece) { "" }, TextToSpeech.QUEUE_FLUSH, Bundle(), id)
        refresh()
    }

    private fun advanceSystem() {
        val it = items.getOrNull(index) ?: return
        if (piece + 1 < it.pieces.size) {
            piece++
        } else if (index + 1 < items.size) {
            index++
            piece = 0
            highlight?.invoke(items[index].id)
        } else {
            playing = false
            unfocus(keep = false)
            refresh()
            return
        }
        // a pause after a heading, as the Apple apps make
        val pause = if (it.kind == "heading" && piece == 0) 350L else 0L
        main.launch { delay(pause); if (playing) speakSystem() }
    }

    // ---- a narration made before (on the Mac, or here) ----

    private var recorder: MediaPlayer? = null
    private var times: List<Pair<Double, Double>> = emptyList()
    private var ticker: Job? = null

    private suspend fun loadRecorded(files: Pair<String, String>, pageItems: List<NarrationSegment>): Boolean = runCatching {
        val json = withContext(Dispatchers.IO) { JSONObject(File(files.second).readText()) }
        val segs = json.getJSONArray("segments")
        if (segs.length() == 0) return false
        val text = pageItems.associate { it.id to it.text }
        val list = ArrayList<Item>()
        val t = ArrayList<Pair<Double, Double>>()
        for (i in 0 until segs.length()) {
            val s = segs.getJSONObject(i)
            val id = s.getString("id")
            list += Item(id, s.optString("kind", "text"), text[id] ?: "", emptyList())
            t += s.getDouble("start") to s.getDouble("end")
        }
        val mp = MediaPlayer()
        mp.setAudioAttributes(attributes)
        withContext(Dispatchers.IO) { mp.setDataSource(files.first); mp.prepare() }
        mp.setOnCompletionListener { playing = false; unfocus(keep = false); refresh() }
        recorder = mp
        items = list
        times = t
        true
    }.getOrDefault(false)

    private fun startTicker() {
        ticker?.cancel()
        ticker = main.launch {
            while (isActive && playing) {
                delay(250)
                val mp = recorder ?: break
                val now = mp.currentPosition / 1000.0
                var lo = 0
                var hi = times.lastIndex
                while (lo < hi) { val mid = (lo + hi + 1) / 2; if (times[mid].first <= now + 0.05) lo = mid else hi = mid - 1 }
                if (lo != index) { index = lo; highlight?.invoke(items[lo].id); refresh() }
            }
        }
    }

    // ---- the natural voice's download ----

    fun refreshStatus() { main.launch { voiceStatus = withContext(Dispatchers.IO) { natural.status() } } }

    fun downloadVoice() {
        if (download != null) return
        download = 0L to (voiceStatus?.downloadBytes?.toLong() ?: 0L)
        main.launch {
            try {
                withContext(Dispatchers.IO) {
                    natural.download(object : DownloadListener {
                        override fun onProgress(done: ULong, total: ULong) { main.launch { download = done.toLong() to total.toLong() } }
                    })
                }
                voiceStatus = withContext(Dispatchers.IO) { natural.status() }
                // a paper being read in the phone's voice goes on in the natural one
                if (items.isNotEmpty() && mode == Mode.System && voiceStatus?.installed == true) {
                    val was = playing
                    pause()
                    mode = Mode.Natural
                    note = null
                    if (was) play(index)
                }
            } catch (e: Exception) {
                if (e.message?.contains("cancelled") != true) note = "Couldn't download the natural voice: ${e.message}"
            } finally {
                download = null
            }
        }
    }

    fun cancelDownload() = natural.cancelDownload()

    fun removeVoice() {
        if (mode == Mode.Natural && items.isNotEmpty()) stop()
        main.launch { voiceStatus = withContext(synth) { runCatching { natural.remove() }; natural.status() } }
    }

    // ---- Make Natural Narration: the whole paper, saved with it (narration.m4a + narration.json) ----

    /** Renders [items] in the natural voice and attaches the narration to the paper; [done] gets an error or null. */
    fun makeNarration(lib: Library, id: String, pageItems: List<NarrationSegment>, done: (String?) -> Unit) {
        if (rendering != null) return
        rendering = Triple(id, 0, pageItems.size)
        main.launch {
            val err = runCatching {
                val dir = File(app.cacheDir, "narration").apply { mkdirs() }
                val wav = File(dir, "$id.wav")
                val m4a = File(dir, "$id.m4a")
                try {
                    val timings = withContext(synth) {
                        natural.render(pageItems, voice, wav.path, object : RenderListener {
                            override fun onProgress(done: UInt, total: UInt) { main.launch { rendering = Triple(id, done.toInt(), total.toInt()) } }
                        })
                    }
                    withContext(Dispatchers.IO) {
                        AacEncoder.encodeWav(wav, m4a)
                        // audio first: the timing map marks the narration as ready (as on the Mac)
                        lib.attachFile(id, FileKind.AUDIO, m4a.readBytes())
                        lib.attachFile(id, FileKind.AUDIO_INDEX, timings.toByteArray())
                    }
                } finally {
                    wav.delete(); m4a.delete()
                }
            }.exceptionOrNull()
            rendering = null
            done(err?.let { it.message ?: "Couldn't make the narration" })
        }
    }

    fun cancelNarration() = natural.cancelDownload()

    // ---- audio focus, headphones unplugged ----

    private var focusRequest: AudioFocusRequest? = null
    private var noisyRegistered = false
    private val noisy = object : BroadcastReceiver() {
        override fun onReceive(c: Context?, i: Intent?) { if (i?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY && playing) pause() }
    }

    private fun focus(): Boolean {
        val am = app.getSystemService(AudioManager::class.java)
        val req = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attributes)
            .setOnAudioFocusChangeListener { change ->
                main.launch {
                    when (change) {
                        AudioManager.AUDIOFOCUS_LOSS, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> if (playing) pause()
                    }
                }
            }
            .build().also { focusRequest = it }
        if (am.requestAudioFocus(req) == AudioManager.AUDIOFOCUS_REQUEST_FAILED) return false
        if (!noisyRegistered) {
            ContextCompat.registerReceiver(app, noisy, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED)
            noisyRegistered = true
        }
        return true
    }

    private fun unfocus(keep: Boolean) {
        if (noisyRegistered) { runCatching { app.unregisterReceiver(noisy) }; noisyRegistered = false }
        if (!keep) focusRequest?.let { app.getSystemService(AudioManager::class.java).abandonAudioFocusRequest(it) }
    }

    // ---- the media session (ListenService) ----

    /** Binds ListenService (it holds the media session; media3 makes it a foreground service while playing). */
    private fun ensureSession() {
        if (controller != null) return
        controller = MediaController.Builder(app, SessionToken(app, ComponentName(app, ListenService::class.java))).buildAsync()
    }

    private fun refresh() { player?.refresh() }
}
