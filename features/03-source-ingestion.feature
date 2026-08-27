@F03 @ingestion
Feature: Normalize source material without losing provenance

  Scenario Outline: Ingest a supported source
    Given the project contains a valid <source>
    When the director ingests it
    Then it records the source path and kind
    And it exposes animation-relevant content to storyboard beats

    Examples:
      | source                                    |
      | Markdown note                             |
      | LaTeX equation                            |
      | PDF document                              |
      | CSV dataset                               |
      | existing Manim Python file                |
      | SVG asset                                 |
      | recorded narration                        |

  Scenario: Report contradictory inputs
    Given two sources assign different values to the same named quantity
    When the director reconciles the inputs
    Then it reports both values and their sources
    And it does not silently choose either value
