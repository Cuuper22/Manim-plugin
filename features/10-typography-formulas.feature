@F10 @typography
Feature: Render text and formulas through the selected engine

  Scenario Outline: Compile supported typography
    Given valid content for <engine>
    And a scene places it inside an allocated region
    When the target scene renders
    Then the pinned <engine> toolchain compiles it
    And the requested media artifact is readable

    Examples:
      | engine |
      | Pango  |
      | LaTeX  |
      | Typst  |

  Scenario: Identify the failing formula
    Given one MathTex expression contains invalid syntax
    When formula compilation fails
    Then the diagnostic includes the compiler cause and available source location
    And healthy expressions are not reported as broken

  Scenario: Diagnose a missing font before choosing a fallback
    Given a render log reports that the preferred font is unavailable
    When the director diagnoses the log
    Then the failure is classified as a font issue
    And the report advises installing the font or selecting a fallback
    And no fallback is reported as used until a revised scene is rendered
