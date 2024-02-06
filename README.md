# target tier docs experiment

Experiment with automatically generating target tier docs.

## Problems

Currenly, the [target tier docs](https://doc.rust-lang.org/rustc/platform-support.html) are hard to navigate.
If you want to find information about a specific target, you first need to do some glob-search yourself and then also hope
that the target actually exists. This is super annoying (`:(`). Additionally, some targets are completely missing and there
is no reason to believe that the documentation won't suddenly start being out of date.
Pages are also inconsistent about which sections exist and which ones don't.

## Solution

Enter: adding yet another preprocessing step.

By adding yet another preprocessing step, we can solve all these problems.
- Have a *dedicated* page for *every single* target including information about maintainers etc. 
  This makes it super easy to find things when there are problems.
- Ensure that no target is completely undocumented, at least having a stub page pointing out the undocumentedness
- Error when there is documentation that is not needed anymore, for example a removed target
- Still keep the nice and easy-to-organize glob structure in the source
- Use a unified structure for all the pages
- This also allows us to put more dynamic values into the docs. For example, I put `--print cfg` there, isn't that pretty!?
