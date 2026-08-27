@F23 @variants @accessibility @localization
Feature: Author and verify accessible multi-aspect variants

  Scenario: Recompose for a vertical profile
    Given the 16 by 9 generalized Fibonacci scene is complete
    When the vertical profile is rendered
    Then semantic beats and displayed values are unchanged
    And composition is relaid out for 9 by 16 rather than cropped
    And the rendered artifact matches the declared vertical dimensions

  Scenario: Render supplied localization work
    Given Arabic narration, captions, and right-to-left-aware source have been authored
    When the Arabic variant is rendered
    Then the authored right-to-left layout is preserved
    And mathematical expressions remain unchanged
    And Director does not claim to have translated unsupplied content

  Scenario: Require non-color meaning
    Given two behaviors differ only by line color
    When Codex performs the accessibility review
    Then the result is not reported as automatically repaired
    And a dash pattern, marker shape, label, or other second encoding is authored before acceptance

  Scenario: Render an explicit reduced-motion variant
    Given the project declares a reduced-motion strategy
    And rapid camera travel has been replaced in source by a cut or short dissolve
    When the reduced-motion variant renders
    Then its content order remains consistent with the primary variant
    And its narration and caption timeline remains valid
