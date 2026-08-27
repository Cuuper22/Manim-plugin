@F08 @vocabulary
Feature: Express the full Manim visual vocabulary

  Scenario Outline: Author an editable visual primitive
    Given a storyboard requests <visual>
    When the director authors the scene
    Then the output uses native or version-compatible Manim objects
    And the visual remains editable in Python source

    Examples:
      | visual                    |
      | equation transformation   |
      | geometric construction    |
      | discontinuous function    |
      | graph traversal            |
      | vector field               |
      | data chart                 |
      | code execution trace       |
      | animated 3D state orbit    |
