@F20 @templates @themes
Feature: Apply executable scene templates and storyboard recipes

  Scenario Outline: Generate an executable scene template
    Given the user selects the <template> scene template
    When the template is generated with a built-in theme
    Then the generated Python parses
    And it declares the documented scene class
    And no unresolved template token survives in the source

    Examples:
      | template                |
      | equation_derivation     |
      | function_explorer       |
      | geometry_proof          |
      | algorithm_walkthrough   |
      | generalized_fibonacci   |

  Scenario Outline: Reuse a bundled storyboard recipe
    Given the user selects the <recipe> storyboard recipe
    When Codex compiles a project brief from it
    Then its semantic beat roles remain present
    And project-specific content remains editable

    Examples:
      | recipe                |
      | intuitive-explainer   |
      | mathematical-proof    |
      | data-story            |
      | algorithm-walkthrough |
      | vertical-short        |

  Scenario: Generate with a selected project theme
    Given a built-in theme defines palette and typography tokens
    When it is selected for template generation
    Then the generated scene uses the selected token values
    And source remains ordinary editable Manim Python
