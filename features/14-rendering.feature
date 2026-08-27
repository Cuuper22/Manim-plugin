@F14 @render
Feature: Render requested scenes and output profiles

  Scenario Outline: Render a declared profile
    Given examples/generalized-fibonacci is valid
    When the <profile> profile renders
    Then the artifact has <width> by <height> pixels at <fps> frames per second
    And its container is readable
    And the job report names the artifact

    Examples:
      | profile      | width | height | fps |
      | preview      | 854   | 480    | 15  |
      | production   | 1920  | 1080   | 60  |
      | vertical     | 1080  | 1920   | 30  |
      | loop-gif     | 640   | 360    | 15  |
      | orbit-3d     | 1280  | 720    | 30  |

  Scenario: Render a transparent deliverable
    Given the transparent profile requests alpha
    When GeneralizedFibonacci renders
    Then the selected container supports alpha
    And the media probe reports an alpha channel

  Scenario: Cancel and retry a render safely
    Given a render is running with partial movie segments on disk
    When the render is cancelled and the same request is submitted again
    Then the complete Manim and FFmpeg process tree stops safely
    And partial segments are never accepted as completed artifacts
    And only a complete validated cache entry may be reused
