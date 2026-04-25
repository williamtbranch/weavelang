Proposed weave format for deep dive study.

This output tts name syntax is <stem>ULsf.txt
We want a way to listen to every sentence through each of its permutations from the base language to target. Henceforth we will assume English and Spanish respectively.

For every sentence in the wvl file we first output the sentence from English original source. If the base language is not in English we will use the basic base tier instead. We likely have not implemented an alternative source language source at this point which is a future plan.

Then we generate (but don't output) each level in steps of 2. Compare the generation to the previous output and if there are differences we output that sentence and continue. We will always generate the last advanced sentence even if it is not a step 2 distance away from the previous. If the highest level shows no differences to the previous output, we don't output it otherwise we do. We don't want duplicates.

Additionally the early levels other than the Source English should be skipped entirely. The reasoning is that there are not too many words to learn up to level 16 (maybe 50). Outputting these levels will produce lots of text the student likely knows already. A student is advised to get comfortable with up to level 16 first before going into using this content. This means that already 50% of the words on the page are in Spanish.

So basically we are outputting levels:
16, 18, 20, 22, 24, 26, 28, 30, 32, 35, ...
Usually 35 to 37 are the highest depending on the reading level of the source text.
We are only outputting levels that show differences from proceeding levels.

*Importantly the frontier flag should be turned off*. The diffusion frontier filter will almost guarantee due to some randomness in the Spanish generation, that every sentence is different and create far too many outputs.

After user testing we may adjust the step size as well, perhaps 3 or 4 is more useful. We want to strike a balance between repetition and comprehension.

