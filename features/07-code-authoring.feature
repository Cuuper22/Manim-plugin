@F07 @authoring
Feature: Author executable and maintainable Manim code

  Scenario: Generate an independently renderable scene
    Given an approved storyboard beat named "state-space"
    When the director authors its scene
    Then the Python parses under the pinned interpreter
    And the Manim CLI discovers the declared scene class
    And a preview render can target that class alone

  Scenario: Preserve manual source during a revision
    Given a scene contains user-authored code outside the requested semantic target
    When the director applies a scoped revision
    Then only the target and its dependent metadata change
    And the user-authored code remains byte-for-byte unchanged

  Scenario: Make visible randomness reproducible
    Given a scene uses random visible positions
    When it renders twice with the same project seed
    Then corresponding sampled frames are visually identical
