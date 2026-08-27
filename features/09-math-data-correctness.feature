@F09 @correctness
Feature: Validate mathematical and data claims

  Scenario: Check the generalized recurrence dataset
    Given examples/generalized-fibonacci/data/sequences.csv
    When each row from n equals 2 onward is validated
    Then value[n] equals p times value[n-1] plus q times value[n-2]
    And every residual is at most 1e-9

  Scenario: Validate an equation transformation
    Given a displayed transformation from "lambda^(n+2)=p lambda^(n+1)+q lambda^n"
    When the common nonzero factor lambda^n is removed
    Then the result is "lambda^2-p lambda-q=0"
    And the nonzero-factor assumption is recorded

  Scenario: Plot a discontinuity honestly
    Given a requested function has poles inside the visible domain
    When the graph is generated
    Then no segment connects points across a pole
    And excluded domain values are represented visually
