@F05 @doctor
Feature: Diagnose a render environment precisely

  Scenario Outline: Report an unavailable capability
    Given the project can be inspected
    And <capability> is unavailable
    When the environment doctor runs
    Then the report does not claim <capability> is available
    And the report names an actionable installation or fallback direction

    Examples:
      | capability       |
      | Manim            |
      | FFmpeg           |
      | LaTeX            |
      | Typst            |

  Scenario: Recommend rather than invent an OpenGL fallback
    Given an OpenGL render log reports a context failure
    When the director diagnoses the log
    Then the failure is classified as a renderer issue
    And Cairo is offered as a possible fallback for a compatible scene
    And a new Cairo render requires an explicit request
