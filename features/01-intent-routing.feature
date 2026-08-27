@F01 @routing
Feature: Route a Manim request to one workflow

  Scenario Outline: Classify a concrete request
    Given a Manim workspace is open
    When the user asks <request>
    Then the director selects the <workflow> workflow
    And no unrelated workflow is scheduled

    Examples:
      | request                                             | workflow |
      | "Animate generalized Fibonacci sequences"          | create   |
      | "Why does MissingAssetScene fail?"                 | debug    |
      | "Make pulse.ring gold"                             | edit     |
      | "Render only the roots section"                    | render   |
      | "Explain TransformMatchingTex in this scene"       | explain  |
      | "Move this ManimGL project to Manim Community"     | migrate  |
