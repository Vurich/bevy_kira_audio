//! Audio plugin for the game engine Bevy
//!
//! It uses the library Kira to play audio and offers an API to control running game audio
//! via Bevy's ECS.
//!
//! ```edition2018
//! # use bevy_kira_audio::{AudioChannel, Audio, AudioPlugin};
//! # use bevy::prelude::*;
//! # use bevy::asset::AssetPlugin;
//! # use bevy::app::AppExit;
//! fn main() {
//!    let mut app = App::new();
//!    app
//!         .add_plugins(MinimalPlugins)
//!         .add_plugin(AssetPlugin)
//!         .add_plugin(AudioPlugin)
//! #       .add_system(stop)
//!         .add_startup_system(start_background_audio);
//!    app.run();
//! }
//!
//! fn start_background_audio(asset_server: Res<AssetServer>, audio: Res<Audio>) {
//!     audio.play_looped(asset_server.load("background_audio.mp3"));
//! }
//!
//! # fn stop(mut events: EventWriter<AppExit>) {
//! #     events.send(AppExit)
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(unused_imports, missing_docs)]
#![feature(const_fn_floating_point_arithmetic)]

pub use audio::{Audio, InstanceHandle, PlaybackState};
pub use channel::AudioChannel;
pub use source::AudioSource;
pub use stream::{AudioStream, Frame, StreamedAudio};

mod audio;
mod audio_output;
mod channel;
mod source;
mod stream;

use crate::audio_output::{
    init_metronome_system, metronome_events_system, play_queued_audio_system, stream_audio_system,
    update_instance_states_system, AudioOutput,
};

#[cfg(feature = "flac")]
use crate::source::FlacLoader;
#[cfg(feature = "mp3")]
use crate::source::Mp3Loader;
#[cfg(feature = "ogg")]
use crate::source::OggLoader;
#[cfg(feature = "settings_loader")]
use crate::source::SettingsLoader;
#[cfg(feature = "wav")]
use crate::source::WavLoader;
use bevy::ecs::system::IntoExclusiveSystem;
use bevy::prelude::{AddAsset, App, CoreStage, Plugin};
use std::marker::PhantomData;

#[cfg(all(
    not(feature = "ogg"),
    not(feature = "mp3"),
    not(feature = "flac"),
    not(feature = "wav")
))]
compile_error!("You need to enable at least one of the bevy_kira_audio features 'ogg', 'mp3', 'flac', or 'wav'");

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum BeatEvent {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    HalfTriplet,
    QuarterTriplet,
    EighthTriplet,
    SixteenthTriplet,
}

const BEAT_EVENTS: [BeatEvent; 9] = [
    BeatEvent::Whole,
    BeatEvent::Half,
    BeatEvent::Quarter,
    BeatEvent::Eighth,
    BeatEvent::Sixteenth,
    BeatEvent::HalfTriplet,
    BeatEvent::QuarterTriplet,
    BeatEvent::EighthTriplet,
    BeatEvent::SixteenthTriplet,
];
const BEAT_SUBDIVISIONS: [f64; 9] = [
    BeatEvent::Whole.to_subdivision(),
    BeatEvent::Half.to_subdivision(),
    BeatEvent::Quarter.to_subdivision(),
    BeatEvent::Eighth.to_subdivision(),
    BeatEvent::Sixteenth.to_subdivision(),
    BeatEvent::HalfTriplet.to_subdivision(),
    BeatEvent::QuarterTriplet.to_subdivision(),
    BeatEvent::EighthTriplet.to_subdivision(),
    BeatEvent::SixteenthTriplet.to_subdivision(),
];

impl BeatEvent {
    const fn to_subdivision(&self) -> f64 {
        match self {
            Self::Whole => 0.,
            Self::Half => 0.5,
            Self::Quarter => 0.25,
            Self::Eighth => 0.125,
            Self::Sixteenth => 0.0625,
            Self::HalfTriplet => 1. / 3.,
            Self::QuarterTriplet => 1. / 6.,
            Self::EighthTriplet => 1. / 12.,
            Self::SixteenthTriplet => 1. / 24.,
        }
    }

    fn from_subdivision(sub: f64) -> Option<Self> {
        let mut i = 0;
        for check_sub in BEAT_SUBDIVISIONS {
            if sub == check_sub {
                return Some(BEAT_EVENTS[i]);
            }

            i += 1;
        }

        return None;
    }
}

#[derive(PartialEq, Eq, Clone)]
pub enum TimelineState {
    Playing,
    Paused,
    Stopped,
}

#[derive(bevy::reflect::TypeUuid, PartialEq, Clone)]
#[uuid = "8979ba65-4ec4-4c5f-8158-c4b3ae22a076"]
pub struct TimelineSettings {
    pub bpm: f64,
    pub state: TimelineState,
}

impl Default for TimelineSettings {
    fn default() -> Self {
        Self {
            bpm: 120.,
            state: TimelineState::Stopped,
        }
    }
}

#[derive(bevy::reflect::TypeUuid, Default)]
#[uuid = "4d582cda-4293-406d-bb76-172bb05be15d"]
pub struct LastTimelineSettings {
    inner: TimelineSettings,
}

/// A Bevy plugin for audio
///
/// Add this plugin to your Bevy app to get access to
/// the Audio resource
/// ```edition2018
/// # use bevy_kira_audio::{AudioChannel, Audio, AudioPlugin};
/// # use bevy::prelude::*;
/// # use bevy::asset::AssetPlugin;
/// # use bevy::app::AppExit;
/// fn main() {
///    let mut app = App::new();
///    app
///         .add_plugins(MinimalPlugins)
///         .add_plugin(AssetPlugin)
///         .add_plugin(AudioPlugin)
/// #       .add_system(stop)
///         .add_startup_system(start_background_audio);
///    app.run();
/// }
///
/// fn start_background_audio(asset_server: Res<AssetServer>, audio: Res<Audio>) {
///     audio.play_looped(asset_server.load("background_audio.mp3"));
/// }
///
/// # fn stop(mut events: EventWriter<AppExit>) {
/// #     events.send(AppExit)
/// # }
/// ```
#[derive(Default)]
pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_non_send_resource::<AudioOutput>()
            .add_asset::<AudioSource>();

        app.add_startup_system(init_metronome_system);

        #[cfg(feature = "mp3")]
        app.init_asset_loader::<Mp3Loader>();
        #[cfg(feature = "ogg")]
        app.init_asset_loader::<OggLoader>();
        #[cfg(feature = "wav")]
        app.init_asset_loader::<WavLoader>();
        #[cfg(feature = "flac")]
        app.init_asset_loader::<FlacLoader>();
        #[cfg(feature = "settings_loader")]
        app.init_asset_loader::<SettingsLoader>();

        app.init_resource::<LastTimelineSettings>()
            .init_resource::<TimelineSettings>()
            .init_resource::<Audio>()
            .add_system_to_stage(CoreStage::PreUpdate, metronome_events_system)
            .add_system_to_stage(CoreStage::PostUpdate, play_queued_audio_system)
            .add_system_to_stage(
                CoreStage::PreUpdate,
                update_instance_states_system.exclusive_system(),
            );
    }
}

/// A Bevy plugin for streaming of audio
///
/// This plugin requires [AudioPlugin] to also be active
/// ```edition2018
/// # use bevy_kira_audio::{AudioStream, Frame, StreamedAudio, AudioChannel, Audio, AudioPlugin, AudioStreamPlugin};
/// # use bevy::prelude::*;
/// # use bevy::asset::AssetPlugin;
/// # use bevy::app::AppExit;
/// fn main() {
///    let mut app = App::new();
///    app
///         .add_plugins(MinimalPlugins)
///         .add_plugin(AssetPlugin)
///         .add_plugin(AudioPlugin)
/// #       .add_system(stop)
///         .add_plugin(AudioStreamPlugin::<SineStream>::default())
///         .add_startup_system(start_stream);
///    app.run();
/// }
///
/// #[derive(Debug, Default)]
/// struct SineStream {
///     t: f64,
///     note: f64,
///     frequency: f64
/// }
///
/// impl AudioStream for SineStream {
///     fn next(&mut self, _: f64) -> Frame {
///         let increment = 2.0 * std::f64::consts::PI * self.note / self.frequency;
///         self.t += increment;
///
///         let sample: f64 = self.t.sin();
///         Frame {
///             left: sample as f32,
///             right: sample as f32,
///         }
///     }
/// }
///
///fn start_stream(audio: Res<StreamedAudio<SineStream>>) {
///    audio.stream(SineStream {
///        t: 0.0,
///        note: 440.0,
///        frequency: 44_000.0,
///    });
///}
///
/// # fn stop(mut events: EventWriter<AppExit>) {
/// #     events.send(AppExit)
/// # }
/// ```
#[derive(Default)]
pub struct AudioStreamPlugin<T: AudioStream> {
    _marker: PhantomData<T>,
}

impl<T> Plugin for AudioStreamPlugin<T>
where
    T: AudioStream,
{
    fn build(&self, app: &mut App) {
        app.init_resource::<StreamedAudio<T>>()
            .add_system_to_stage(CoreStage::PostUpdate, stream_audio_system::<T>);
    }
}

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
struct ReadmeDoctests;
