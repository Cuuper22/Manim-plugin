@F16 @qa
Feature: Inspect rendered frames and repair visible defects

  Scenario Outline: Detect an implemented visual signal
    Given a rendered sample contains <defect>
    When visual QA inspects the sample
    Then the report records a severity and supporting frame metrics
    And the report classifies the signal as <kind>

    Examples:
      | defect                                      | kind           |
      | a blank or nearly uniform frame             | blank_frame    |
      | low frame-wide luminance separation         | low_contrast   |
      | visible content entering the safe margin    | safe_area      |
      | an annotated object outside the frame       | object_clipped |
      | two annotated objects overlapping materially | object_overlap |

  Scenario: Bound Codex-directed repair
    Given a concrete visual defect remains after two scoped repair passes
    When the last repaired preview is inspected
    Then the director stops automatic repair
    And it reports the affected scene or section, evidence, and attempted patches
