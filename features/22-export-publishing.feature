@F22 @export
Feature: Export a reproducible animation project

  Scenario: Package the generalized Fibonacci project
    Given final rendering and required QA have succeeded
    When a bundle export is requested
    Then the bundle contains editable source, director.yaml, manim.cfg, assets, data, themes, narration, captions, and media
    And its manifest enumerates the archived project files
    And generated cache and secret files are absent

  Scenario Outline: Export a supported media format
    Given a readable project-contained source video exists
    When the <format> export format is selected
    Then the exported artifact is readable in the requested container
    And the export report names its source and destination

    Examples:
      | format |
      | mp4    |
      | webm   |
      | gif    |

  Scenario: Package caption sidecars
    Given valid timed VTT or SRT cues exist
    When a captions export is requested
    Then the archive contains normalized VTT and SRT sidecars
    And the export manifest records cue count and duration
