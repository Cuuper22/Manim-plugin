@F13 @audio @captions
Feature: Synchronize narration, audio, and captions

  Scenario: Reconcile narration timing with storyboard beats
    Given ordered storyboard beats and timed narration cues
    When caption timing is reconciled
    Then every returned beat has explicit start, end, and duration values
    And beats extend rather than truncate longer narration cues

  Scenario: Export caption sidecars
    Given timed English narration exists
    When captions are exported
    Then VTT and SRT cues are ordered and non-overlapping
    And mathematical notation remains intelligible in plain text

  Scenario: Mix narration with background audio
    Given narration and music tracks overlap
    And their start times and gains are explicit
    When the audio tracks are mixed
    Then the output preserves the declared offsets and gains
    And the mixed audio is readable by the media probe

  Scenario: Normalize a narration track
    Given a narration track is readable by FFmpeg
    When loudness normalization is requested with a target LUFS
    Then FFmpeg creates a distinct normalized output
    And the normalized output is readable by the media probe
