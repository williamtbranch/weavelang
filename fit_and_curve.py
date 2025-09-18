import pandas as pd
import numpy as np
from scipy.optimize import curve_fit
import matplotlib.pyplot as plt
import io

# 1. Prepare the data from your AVD Hunter run
# I've embedded the data you provided directly into the script.
csv_data = """user_level,v_level_boundary,avd_score
1,1,0.6667
2,2,1.6667
3,3,1.6667
4,4,1.6667
5,5,2.3333
6,7,4.0
7,8,4.6667
8,12,6.3333
9,15,8.0
10,17,11.3333
11,21,13.0
12,28,14.0
13,38,16.6667
14,52,21.3333
15,69,27.6667
16,92,35.6667
17,113,48.6667
18,155,63.0
19,195,82.0
20,253,106.6667
21,329,136.0
22,454,181.0
23,598,226.3333
24,774,322.0
25,867,415.0
26,1031,529.6667
27,1350,634.0
28,1797,783.0
29,2434,990.6667
30,2654,1311.3333
31,3708,1774.3333
32,5492,1991.6667
33,7777,2740.3333
34,11497,3620.0
35,25428,5376.3333
36,201655,7274.3333
37,2432326,12253.0
38,3648489,12772.0
"""

df = pd.read_csv(io.StringIO(csv_data))

# --- Data Preparation ---
# The last data point (level 38) is an outlier because the hunt ended prematurely
# (it couldn't find 2% new density). We should exclude it from the fit for accuracy.
df_filtered = df[df['user_level'] < 38]

x_data = df_filtered['avd_score'].values
y_data = df_filtered['user_level'].values

# 2. Define the logarithmic function we want to fit.
#    Model: y = a * log(x + 1) + b
#    We use log(x + 1) to handle the AVD score of 0 at the beginning.
def log_func(x, a, b):
    return a * np.log(x + 1) + b

# 3. Perform the curve fitting using scipy.
#    This finds the optimal values for 'a' and 'b'.
try:
    params, covariance = curve_fit(log_func, x_data, y_data)
    a_fit, b_fit = params
    print("--- Curve Fit Results ---")
    print(f"Discovered function: user_level = {a_fit:.4f} * log(avd_score + 1) + {b_fit:.4f}")
    print("-" * 25)

    # 4. Create the final, usable functions based on the discovered parameters
    def get_user_level_from_avd(avd_score):
        """Calculates the user level for a given AVD score using the fitted curve."""
        level = a_fit * np.log(avd_score + 1) + b_fit
        return max(1.0, level) # Ensure level is at least 1

    def get_avd_from_user_level(user_level):
        """Calculates the target AVD score for a given user level (inverse function)."""
        # Solved for x from y = a * log(x + 1) + b
        avd_score = np.exp((user_level - b_fit) / a_fit) - 1
        return max(0.0, avd_score)

    # 5. Display a smoothed, idealized Master AVD Scale generated from our new function
    print("\n--- Smoothed Master AVD Scale (from formula) ---")
    print(f"{'User Level':<12} | {'Target AVD Score':<20}")
    print("-" * 35)
    for level in range(1, 41):
        target_avd = get_avd_from_user_level(level)
        print(f"{level:<12} | {target_avd:<20.2f}")


    # 6. Visualize the results to confirm the fit is good
    plt.figure(figsize=(12, 8))
    # Plot the original data points
    plt.scatter(x_data, y_data, label='Hunter Data Points', color='red', zorder=5)

    # Generate smooth data for the fitted curve
    x_smooth = np.linspace(min(x_data), max(x_data), 500)
    y_smooth = log_func(x_smooth, a_fit, b_fit)

    # Plot the fitted curve
    plt.plot(x_smooth, y_smooth, label=f'Fitted Curve\ny={a_fit:.2f}*log(x+1)+{b_fit:.2f}', color='blue', linewidth=2)

    plt.title('User Level vs. AVD Score with Logarithmic Fit', fontsize=16)
    plt.xlabel('AVD Score (Density-Weighted)', fontsize=12)
    plt.ylabel('User Level', fontsize=12)
    plt.legend()
    plt.grid(True)
    plt.show()

except RuntimeError:
    print("Curve fit failed. This can happen if the data is not well-suited for the model.")
except ImportError:
    print("Error: Required libraries not found. Please run 'pip install scipy pandas numpy matplotlib'")