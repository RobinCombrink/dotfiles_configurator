Feature: Elevating what the machine refuses without elevation

  Windows refuses a symlink to an apply that holds neither elevation nor Developer Mode, and some
  installers refuse to start without elevation. An apply collects whatever a pass is refused this
  way and runs all of it together elevated at the end of that pass, so a pass asks for elevation at
  most once.

  Scenario: A link refused for want of a privilege is made once Alice allows elevation
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And Alice's machine refuses links for want of a privilege
    And Alice allows elevation when asked
    When Alice applies
    Then the link ".gitconfig" points into the dotfiles repository
    And the machine is reported as converged

  Scenario: An installer that demands elevation is run once Alice allows elevation
    Given Alice declares the application "Steam"
    And Steam is not installed on Alice's machine
    And the installer for Steam demands elevation on Alice's machine
    And Alice allows elevation when asked
    When Alice applies
    Then Steam is installed on Alice's machine
    And the machine is reported as converged

  Scenario: Everything a pass is refused without elevation is elevated under one prompt
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And Alice declares the symlink "npm/.npmrc" at ".npmrc"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And the dotfiles repository holds "npm/.npmrc"
    And the dotfiles repository has been cloned on Alice's machine
    And Alice declares the application "Steam"
    And Steam is not installed on Alice's machine
    And the installer for Steam demands elevation on Alice's machine
    And Alice's machine refuses links for want of a privilege
    And Alice allows elevation when asked
    When Alice applies
    Then Alice was asked for elevation 1 time

  Scenario: An entry waiting on elevation is reported once, when the elevated run settles it
    Given Alice declares the application "Steam"
    And Steam is not installed on Alice's machine
    And the installer for Steam demands elevation on Alice's machine
    And Alice allows elevation when asked
    When Alice applies
    Then Alice's run reports "Steam" as "installing" and then as "installed"

  Scenario: Declining elevation holds everything that needed it
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And Alice declares the application "Steam"
    And Steam is not installed on Alice's machine
    And the installer for Steam demands elevation on Alice's machine
    And Alice's machine refuses links for want of a privilege
    And Alice declines elevation when asked
    When Alice applies
    Then 2 resources are reported as held
    And 0 resources are reported as failed
    And Alice's run shows "Steam" as held because "elevation declined"

  Scenario: Declined elevation is asked for once rather than on every pass
    Given Alice declares the application "Steam"
    And Steam is not installed on Alice's machine
    And the installer for Steam demands elevation on Alice's machine
    And Alice declines elevation when asked
    And Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies
    Then Neovim is installed on Alice's machine
    And Alice was asked for elevation 1 time

  Scenario: An entry the elevated run fails is reported failed without failing the rest
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And Alice's machine refuses links for want of a privilege
    And Alice declares the application "Steam"
    And Steam is not installed on Alice's machine
    And the installer for Steam demands elevation on Alice's machine
    And installing Steam fails on Alice's machine
    And Alice allows elevation when asked
    When Alice applies
    Then 1 resource is reported as failed
    And the link ".gitconfig" points into the dotfiles repository

  Scenario: An apply already elevated reports a refused link as failed rather than asking again
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And Alice's machine refuses links for want of a privilege
    And Alice's apply is already elevated
    When Alice applies
    Then 1 resource is reported as failed
    And the run reports a failure mentioning "os error 1314"
    And Alice was asked for elevation 0 times
