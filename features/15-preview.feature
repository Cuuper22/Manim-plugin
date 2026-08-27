@F15 @preview
Feature: Preview the smallest invalidated scope

  Scenario: Return one requested named section
    Given the scene declares section "roots"
    When a section preview for "roots" is requested
    Then the result exposes only artifacts that match "roots"
    And Manim partial movie segments are not exposed as preview artifacts

  Scenario: Build a representative contact sheet
    Given a completed preview video exists
    When a contact sheet is requested
    Then it contains bounded representative frames from across the video
    And every extracted frame records its timestamp

  Scenario: Preview captions without production rendering
    Given timed caption cues exist
    And a preview video is open in the workbench
    When the playhead enters a caption cue
    Then the workbench overlays that cue on the preview
    And no production render is scheduled
