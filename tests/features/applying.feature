Feature: Applying a change set

  Applying enacts a change set and keeps going until a pass changes nothing, because whether a
  resource can be converged is read from the machine rather than assumed from order.

  Scenario: Applying to a machine with no drift changes nothing
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice applies
    Then nothing on Alice's machine has changed
    And the machine is reported as converged

  Scenario: A symlink waits for the dotfiles repository and converges once it is cloned
    Given Alice declares the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And the dotfiles repository has not been cloned on Alice's machine
    When Alice applies
    Then the dotfiles repository is cloned on Alice's machine
    And the link ".gitconfig" points into the dotfiles repository
    And the machine is reported as converged

  Scenario: A resource that fails to converge does not stop the ones declared after it
    Given Alice declares the application "Broken"
    And Alice declares the application "Discord"
    And Broken is not installed on Alice's machine
    And Discord is not installed on Alice's machine
    And installing Broken fails on Alice's machine
    When Alice applies
    Then Discord is installed on Alice's machine
    And 1 resource is reported as failed
    And the machine is not reported as converged

  Scenario: A resource that cannot be read leaves the machine reported as unconverged
    Given Alice declares the winget package "Microsoft.PowerShell"
    And winget is absent from Alice's machine
    When Alice applies
    Then 1 resource is reported as blocked
    And the machine is not reported as converged

  Scenario: Withdrawing a declaration leaves what it created on the machine
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice withdraws the declaration of "Neovim"
    And Alice applies
    Then Neovim is still installed on Alice's machine
    And the machine is reported as converged

  Scenario: A failing resource is attempted once rather than again and again
    Given Alice declares the application "Broken"
    And Broken is not installed on Alice's machine
    And installing Broken fails on Alice's machine
    When Alice applies
    Then installing Broken was attempted 1 time

  Scenario: An installer that reports success without installing is not called converged
    Given Alice declares the application "Silent"
    And Silent is not installed on Alice's machine
    And installing Silent reports success without installing anything
    When Alice applies
    Then the machine is not reported as converged
    And 1 resource is reported as not having taken

  Scenario: A real file where a link should go is reported rather than deleted
    Given Alice declares the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And Alice already has a file of her own at ".gitconfig"
    When Alice applies
    Then Alice's own file at ".gitconfig" is still there
    And 1 resource is reported as failed
