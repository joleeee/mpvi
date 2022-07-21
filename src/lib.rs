use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

mod sock;
use sock::{Command, MpvError, MpvMsg, MpvSocket};

mod handle;
pub use handle::MpvHandle as Mpv;
pub use handle::option;

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "event", rename_all = "kebab-case")]
// TODO: add the fields for the events that have them
// https://mpv.io/manual/master/#list-of-events
pub enum Event {
    StartFile,
    EndFile,
    FileLoaded,
    Seek,
    PlaybackRestart,
    Shutdown,
    LogMessage,
    Hook,
    GetPropertyReply,
    SetPropertyReply,
    CommandReply,
    ClientMessage,
    VideoReconfig,
    AudioReconfig,
    PropertyChange,

    // Why are these undocumented?
    Pause,
    Unpause,
}

#[derive(Serialize, EnumString, Display, EnumIter, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
// TODO: add all
// TODO: how can me properly match property name -> output type? maybe a macro...
// https://mpv.io/manual/master/#properties
pub enum Property {
    AudioSpeedCorrection,
    VideoSpeedCorrection,
    DisplaySyncActive,
    Filename,
    FileSize,
    EstimatedFrameCount,
    EstimatedFrameNumber,
    Pid,
    Path,
    StreamOpenFilename,
    MediaTitle,
    FileFormat,
    CurrentDemuxer,
    StreamPath,
    StreamPos,
    StreamEnd,
    Duration,
    Avsync,
    TotalAvsyncChange,
    DecoderFrameDropCount,
    FrameDropCount,
    MistimedFrameCount,
    VsyncRatio,
    VoDelayedFrameCount,
    PercentPos,
    TimePos,
    TimeStart,
    TimeRemaining,
    AudioPts,
    PlaytimeRemaining,
    PlaybackTime,
    Chapter,
    Edition,
    CurrentEdition,
    Chapters,
    Editions,
    EditionList,
    Metadata,
    FilteredMetadata,
    ChapterMetadata,
    //vf-metadata/<filter-label>
    //af-metadata/<filter-label>
    IdleActive,
    CoreIdle,
    CacheSpeed,
    DemuxerCacheDuration,
    DemuxerCacheTime,
    DemuxerCacheIdle,
    DemuxerCacheState,
    DemuxerViaNetwork,
    DemuxerStartTime,
    PausedForCache,
    CacheBufferingState,
    EofReached,
    Seeking,
    MixerActive,
    AoVolume,
    AoMute,
    AudioCodec,
    AudioCodecName,
    AudioParams,
    AudioOutParams,
    Colormatrix,
    ColormatrixInputRange,
    ColormatrixPrimaries,
    Hwdec,
    HwdecCurrent,
    VideoFormat,
    VideoCodec,
    Width,
    Height,
    VideoParams,
    Dwidth,
    Dheight,
    VideoDecParams,
    VideoOutParams,
    VideoFrameInfo,
    ContainerFps,
    EstimatedVfFps,
    WindowScale,
    CurrentWindowScale,
    Focused,
    DisplayNames,
    DisplayFps,
    EstimatedDisplayFps,
    VsyncJitter,
    DisplayWidth,
    DisplayHeight,
    DisplayHidpiScale,
    VideoAspect,
    OsdWidth,
    OsdHeight,
    OsdPar,
    OsdDimensions,
    MousePos,
    SubText,
    SubTextAss,
    SecondarySubText,
    SubStart,
    SecondarySubStart,
    SubEnd,
    SecondarySubEnd,
    PlaylistPos,
    #[serde(rename = "playlist-pos-1")]
    #[strum(serialize = "playlist-pos-1")]
    PlaylistPos1,
    PlaylistCurrentPos,
    PlaylistPlayingPos,
    PlaylistCount,
    Playlist,
    TrackList,
    //current-tracks/..
    ChapterList,
    //Av, not found
    Vf,
    Seekable,
    PartiallySeekable,
    PlaybackAbort,
    CursorAutohide,
    // OsdSymCc, INVALID UTF-8
    // OsdAssCc,
    VoConfigured,
    VoPasses,
    PerfInfo,
    VideoBitrate,
    AudioBitrate,
    SubBitrate,
    PacketVideoBitrate,
    PacketAudioBitrate,
    PacketSubBitrate,
    AudioDeviceList,
    AudioDevice,
    CurrentVo,
    CurrentAo,
    SharedScriptProperties,
    WorkingDirectory,
    ProtocolList,
    DecoderList,
    EncoderList,
    DemuxerLavfList,
    InputKeyList,
    MpvVersion,
    MpvConfiguration,
    FfmpegVersion,
    LibassVersion,
    //options/<name>
    //file-local-options/<name>
    //option-info/<name>
    PropertyList,
    ProfileList,
    CommandList,
    InputBindings,

    // why is this not documented lol
    Pause,
}

#[cfg(test)]
mod tests {
    use super::handle::option;
    use super::*;
    use strum::IntoEnumIterator;
    use tokio::{
        sync::mpsc,
        time::{sleep, Duration},
    };
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn mpv_test() {
        let handle = Mpv::new("/tmp/mpv.sock").await.unwrap();

        let (tx, rx) = mpsc::channel(8);
        handle.subscribe_events(tx).await.unwrap();
        tokio::spawn(print_events(rx));

        println!("Pausing...");
        handle.pause().await.unwrap();
        println!("Paused? {}", handle.get_pause().await.unwrap());
        sleep(Duration::from_millis(1000)).await;

        println!("Unpausing...");
        handle.unpause().await.unwrap();
        println!("Paused? {}", handle.get_pause().await.unwrap());
        sleep(Duration::from_millis(1000)).await;

        println!("Pausing...");
        handle.pause().await.unwrap();
        println!("Paused? {}", handle.get_pause().await.unwrap());

        // have to wait for the message to be sent (until we add in waiting for awck or someting)
        sleep(Duration::from_millis(100)).await;
    }

    async fn print_events(mut rx: mpsc::Receiver<Event>) {
        while let Some(event) = rx.recv().await {
            println!("event {:?}", event);
        }
    }

    #[tokio::test]
    #[serial]
    async fn seek_test() {
        let handle = Mpv::new("/tmp/mpv.sock").await.unwrap();

        use option::Seek::*;
        let modes = [Absolute, Relative, AbsolutePercent, RelativePercent];

        for m in &modes {
            handle.seek(20_u32, *m).await.unwrap();
            sleep(Duration::from_millis(200)).await
        }

        for m in &modes {
            handle.seek(1f32, *m).await.unwrap();
            sleep(Duration::from_millis(50)).await
        }
    }

    #[tokio::test]
    #[serial]
    async fn property_test() {
        let handle = Mpv::new("/tmp/mpv.sock").await.unwrap();
        for property in Property::iter() {
            let res = handle.get_property(property).await;
            if let Err(e) = res {
                match e {
                    // acceptable error
                    handle::HandleError::MpvError(e) => assert_eq!(e.0, "property unavailable"),
                    handle::HandleError::RecvError(e) => Err(e).unwrap(),
                }
            }
        }
    }

    #[test]
    #[serial]
    fn ser_test() {
        for property in Property::iter() {
            let strum_txt = property.to_string();
            let serde_txt = serde_json::to_string(&property).unwrap();
            let serde_txt = serde_txt.trim_matches('"');

            assert_eq!(strum_txt, serde_txt);
        }
    }
}
