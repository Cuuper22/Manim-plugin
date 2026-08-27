@F02 @brief
Feature: Compile a complete animation brief

  Scenario: Infer harmless defaults
    Given the request is "Animate generalized Fibonacci for curious adults"
    And duration, renderer, and resolution are absent
    When the director compiles the brief
    Then the brief records explicit defaults for each absent field
    And authoring continues without a blocking question

  Scenario: Ask once when rigor changes the product
    Given the request can reasonably mean either a formal proof or an intuitive overview
    And the audience does not resolve that choice
    When the director compiles the brief
    Then it asks one question choosing the rigor level
    And it does not author scenes before that answer
