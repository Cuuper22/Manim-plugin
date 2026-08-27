@F06 @scaffold
Feature: Scaffold a maintainable Manim project

  Scenario: Create a project from a compiled brief
    Given a valid brief named "recurrence-lab"
    When project_init scaffolds the project
    Then director.yaml, manim.cfg, requirements, source, asset, output, and state paths exist
    And theme values and deterministic seed are centralized
    And the starter scene is ordinary editable Manim Python

  Scenario: Refuse to overwrite an occupied destination
    Given the destination contains user files
    When project_init scaffolds without force or an explicit merge request
    Then no existing file changes
    And the conflict names the occupied destination
