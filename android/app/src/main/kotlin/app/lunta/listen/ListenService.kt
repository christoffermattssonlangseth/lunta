package app.lunta.listen

import android.app.PendingIntent
import android.content.Intent
import android.os.Looper
import androidx.annotation.OptIn
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import app.lunta.MainActivity
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * Keeps Listen going in the background: the media session for [Narrator] (the media notification,
 * the lock screen, headset and Bluetooth buttons, Android Auto's and watches' controls). media3 runs
 * it as a foreground service of type mediaPlayback while the narration plays.
 */
class ListenService : MediaSessionService() {
    private var session: MediaSession? = null

    @OptIn(UnstableApi::class)
    override fun onCreate() {
        super.onCreate()
        Narrator.init(this)
        val player = NarrationPlayer(mainLooper)
        Narrator.player = player
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        session = MediaSession.Builder(this, player).setSessionActivity(open).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    /** Swiped away from the recent apps while paused: stop; while playing it goes on. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        if (!Narrator.playing) {
            Narrator.stop()
            stopSelf()
        }
    }

    override fun onDestroy() {
        session?.let { s ->
            s.player.release()
            s.release()
        }
        session = null
        Narrator.player = null
        super.onDestroy()
    }
}

/**
 * The narration as media3 sees it: one media item per narration item (so next and previous are
 * paragraphs), the paper's title and the section being read as its metadata.
 */
@OptIn(UnstableApi::class)
class NarrationPlayer(looper: Looper) : SimpleBasePlayer(looper) {
    private val commands = Player.Commands.Builder().addAll(
        Player.COMMAND_PLAY_PAUSE, Player.COMMAND_STOP,
        Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_MEDIA_ITEM, Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
        Player.COMMAND_GET_TIMELINE, Player.COMMAND_GET_METADATA, Player.COMMAND_SET_SPEED_AND_PITCH,
        Player.COMMAND_RELEASE,
    ).build()

    /** Narrator changed: media3 reads the state again (on the main thread). */
    fun refresh() = invalidateState()

    override fun getState(): State {
        val n = Narrator
        val b = State.Builder().setAvailableCommands(commands)
        if (n.items.isEmpty()) return b.setPlaybackState(Player.STATE_IDLE).build()
        val playlist = n.items.indices.map { i ->
            MediaItemData.Builder("narration-$i")
                .setMediaMetadata(
                    MediaMetadata.Builder()
                        .setTitle(n.title.ifEmpty { "Listening" })
                        .setArtist(n.sectionAt(i).ifEmpty { "Lunta" })
                        .setAlbumTitle("Lunta")
                        .build(),
                )
                .build()
        }
        return b.setPlaylist(playlist)
            .setCurrentMediaItemIndex(n.index.coerceIn(0, playlist.lastIndex))
            .setPlayWhenReady(n.playing, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(if (n.waiting) Player.STATE_BUFFERING else Player.STATE_READY)
            .setPlaybackParameters(PlaybackParameters(n.speed))
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) Narrator.resume() else Narrator.pause()
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        if (mediaItemIndex >= 0) Narrator.play(mediaItemIndex)
        return Futures.immediateVoidFuture()
    }

    override fun handleSetPlaybackParameters(playbackParameters: PlaybackParameters): ListenableFuture<*> {
        Narrator.changeSpeed(playbackParameters.speed)
        return Futures.immediateVoidFuture()
    }

    override fun handleStop(): ListenableFuture<*> {
        Narrator.stop()
        return Futures.immediateVoidFuture()
    }

    override fun handleRelease(): ListenableFuture<*> = Futures.immediateVoidFuture()
}
